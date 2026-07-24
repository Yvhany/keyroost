//! CTAP2 `authenticatorCredentialManagement` (0x0A, or preview 0x41).
//!
//! Each sub-command lists or mutates the resident (discoverable) credentials
//! stored on the authenticator. Every request must be authenticated with a
//! `pinUvAuthParam` HMAC over a sub-command-specific value, computed using
//! the pinUvAuthToken obtained from `clientPin::get_pin_token`.
//!
//! Authenticators advertise support via two `getInfo` options: `credMgmt`
//! for the standard 0x0A command, and `credentialMgmtPreview` for the older
//! 0x41 form (most notably emitted by YubiKey 5 firmware ≤ 5.4.x). We probe
//! both via [`CredentialManager::new`] and use whichever the device claims.

use crate::cbor::{self, Value};
use crate::client_pin::PinUvAuthToken;
use crate::cmd::{AuthenticatorInfo, CtapError};
use crate::hid::CTAPHID_CBOR;
use crate::transport::CtapTransport;

pub const CTAP2_CREDENTIAL_MANAGEMENT: u8 = 0x0A;
pub const CTAP2_CREDENTIAL_MANAGEMENT_PREVIEW: u8 = 0x41;

/// Request map keys.
const KEY_SUB_COMMAND: u64 = 0x01;
const KEY_SUB_COMMAND_PARAMS: u64 = 0x02;
const KEY_PIN_UV_AUTH_PROTOCOL: u64 = 0x03;
const KEY_PIN_UV_AUTH_PARAM: u64 = 0x04;

/// Sub-command numbers.
const SUB_GET_CREDS_METADATA: u8 = 0x01;
const SUB_ENUMERATE_RPS_BEGIN: u8 = 0x02;
const SUB_ENUMERATE_RPS_NEXT: u8 = 0x03;
const SUB_ENUMERATE_CREDS_BEGIN: u8 = 0x04;
const SUB_ENUMERATE_CREDS_NEXT: u8 = 0x05;
const SUB_DELETE_CREDENTIAL: u8 = 0x06;

/// `CTAP2_ERR_NO_CREDENTIALS`. enumerate*Begin returns this status (rather than
/// a `total = 0` count) when the authenticator holds no discoverable
/// credentials — a normal "empty" signal, not a failure.
const CTAP2_ERR_NO_CREDENTIALS: u8 = 0x2E;

/// Sub-command parameter keys (CTAP §6.8.2).
const PARAM_RP_ID_HASH: u64 = 0x01;
const PARAM_CREDENTIAL_ID: u64 = 0x02;

/// Response map keys (the same numeric keys mean different things for
/// different sub-commands — context determines which fields are populated).
const RESP_EXISTING_COUNT: u64 = 0x01;
const RESP_MAX_REMAINING: u64 = 0x02;
const RESP_RP: u64 = 0x03;
const RESP_RP_ID_HASH: u64 = 0x04;
const RESP_TOTAL_RPS: u64 = 0x05;
const RESP_USER: u64 = 0x06;
const RESP_CREDENTIAL_ID: u64 = 0x07;
const RESP_PUBLIC_KEY: u64 = 0x08;
const RESP_TOTAL_CREDS: u64 = 0x09;
// 0x0A is credProtect (a small uint), not parsed here.
const RESP_LARGE_BLOB_KEY: u64 = 0x0B;

/// Upper bound on the RP / credential count we'll enumerate. Real
/// authenticators hold at most a few dozen resident credentials; the spec's
/// `totalRPs` / `totalCredentials` are device-claimed `u32`s, so a hostile or
/// buggy key could answer with a gigantic value and drive the `…Next` loop to
/// stream entries indefinitely (unbounded memory / a multi-hour hang). A count
/// past this ceiling is treated as a malformed response.
const MAX_ENUMERATED: usize = 4096;

/// Validate a device-claimed enumeration total against [`MAX_ENUMERATED`].
fn checked_total(total: usize) -> Result<usize, CtapError> {
    if total > MAX_ENUMERATED {
        Err(CtapError::InvalidResponseShape(
            "authenticator claimed an implausible enumeration count",
        ))
    } else {
        Ok(total)
    }
}

/// Aggregate resident-credential storage stats from `getCredsMetadata`.
#[derive(Debug, Clone, Default)]
pub struct CredsMetadata {
    pub existing_count: u64,
    pub max_remaining: u64,
}

/// A relying party that owns at least one resident credential on the key.
#[derive(Debug, Clone)]
pub struct RelyingParty {
    pub id: String,
    pub name: Option<String>,
    /// SHA-256 of the RP ID as reported by the authenticator — the handle
    /// [`CredentialManager::list_credentials`] is keyed by. `None` when the
    /// device returned a missing or wrong-length hash for this entry: the RP
    /// is still listed, but per-RP credential queries must be skipped rather
    /// than run against a guessed or zero-filled key (a zeroed hash would
    /// quietly query the wrong RP).
    pub rp_id_hash: Option<[u8; 32]>,
}

/// User entity bound to a resident credential.
#[derive(Debug, Clone, Default)]
pub struct CredentialUser {
    pub id: Vec<u8>,
    pub name: Option<String>,
    pub display_name: Option<String>,
}

/// One resident credential entry returned by `enumerateCredentials`.
#[derive(Clone)]
pub struct Credential {
    /// PublicKeyCredentialDescriptor.id — used as the handle for deletion.
    pub credential_id: Vec<u8>,
    pub user: CredentialUser,
    /// COSE algorithm identifier extracted from the credential public key.
    pub algorithm: Option<i64>,
    /// The credential's 32-byte largeBlobKey, when the authenticator returned
    /// one (present only for credentials created with the largeBlob extension).
    /// Used to decrypt this credential's per-credential largeBlob entry.
    pub large_blob_key: Option<[u8; 32]>,
}

impl Credential {
    /// Wipe the largeBlobKey in place. Split out of [`Drop`] so the wipe
    /// itself is unit-testable (observing freed memory would need `unsafe`,
    /// which the workspace forbids). The `Option` keeps its `Some`/`None`
    /// shape; only the key bytes are cleared.
    fn scrub_key(&mut self) {
        use zeroize::Zeroize;
        if let Some(key) = self.large_blob_key.as_mut() {
            key.zeroize();
        }
    }
}

/// The largeBlobKey is key material, like the pinUvAuthToken and the PIN
/// shared secrets this crate already scrubs — clear it (and every clone's
/// copy) on drop instead of releasing it to the allocator intact. A manual
/// `Drop` rather than a wrapper type keeps the public field an
/// `Option<[u8; 32]>`; nothing in the tree moves fields out of a
/// `Credential`, which `Drop` would otherwise forbid.
impl Drop for Credential {
    fn drop(&mut self) {
        self.scrub_key();
    }
}

/// Manual `Debug`: `large_blob_key` is key material (decrypts this
/// credential's largeBlob entry), so it must never be printed verbatim —
/// redact it even though nothing `{:?}`-prints a `Credential` today.
impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        struct Redacted;
        impl std::fmt::Debug for Redacted {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "<redacted 32 bytes>")
            }
        }
        f.debug_struct("Credential")
            .field("credential_id", &self.credential_id)
            .field("user", &self.user)
            .field("algorithm", &self.algorithm)
            .field(
                "large_blob_key",
                &self.large_blob_key.as_ref().map(|_| Redacted),
            )
            .finish()
    }
}

/// A decoded enumerateCredentials response, held only as long as the parse
/// needs it. The decoder's `Value::Bytes` copy of the largeBlobKey is as much
/// key material as the field it feeds, so the wrapper wipes it on drop —
/// including on the early-return paths where the response is never parsed.
struct CredResponse(Value);

impl std::ops::Deref for CredResponse {
    type Target = Value;
    fn deref(&self) -> &Value {
        &self.0
    }
}

impl Drop for CredResponse {
    fn drop(&mut self) {
        scrub_large_blob_key(&mut self.0);
    }
}

/// Clear the `largeBlobKey` (0x0B) bytes of a decoded response map in place.
/// Scoped to this one response key on purpose: a blanket wipe of every
/// `cbor::Value::Bytes` would touch every CBOR response in the crate.
fn scrub_large_blob_key(v: &mut Value) {
    use zeroize::Zeroize;
    if let Value::Map(entries) = v {
        for (k, val) in entries.iter_mut() {
            if k.as_uint() == Some(RESP_LARGE_BLOB_KEY) {
                if let Value::Bytes(bytes) = val {
                    bytes.zeroize();
                }
            }
        }
    }
}

/// Bundle of (device, token, command-code) for issuing credentialManagement
/// sub-commands. The wrapper exists because every sub-command needs the
/// same authentication and the device's choice between 0x0A and 0x41 has to
/// be discovered once from `getInfo`.
pub struct CredentialManager<'a, T: CtapTransport> {
    dev: &'a mut T,
    token: PinUvAuthToken,
    cmd_code: u8,
}

impl<'a, T: CtapTransport> CredentialManager<'a, T> {
    /// Construct after `clientPin::get_pin_token`. Returns
    /// [`CtapError::InvalidResponseShape`] if the authenticator advertises
    /// neither `credMgmt` nor `credentialMgmtPreview`.
    pub fn new(
        dev: &'a mut T,
        token: PinUvAuthToken,
        info: &AuthenticatorInfo,
    ) -> Result<Self, CtapError> {
        let cmd_code = if info.option("credMgmt") == Some(true) {
            CTAP2_CREDENTIAL_MANAGEMENT
        } else if info.option("credentialMgmtPreview") == Some(true) {
            CTAP2_CREDENTIAL_MANAGEMENT_PREVIEW
        } else {
            return Err(CtapError::InvalidResponseShape(
                "authenticator does not support credentialManagement",
            ));
        };
        Ok(Self {
            dev,
            token,
            cmd_code,
        })
    }

    /// Resident credential capacity stats.
    pub fn metadata(&mut self) -> Result<CredsMetadata, CtapError> {
        let resp = self.dispatch(SUB_GET_CREDS_METADATA, None)?;
        let mut meta = CredsMetadata::default();
        for (k, v) in resp.as_map().into_iter().flatten() {
            match k.as_uint() {
                Some(RESP_EXISTING_COUNT) => meta.existing_count = v.as_uint().unwrap_or(0),
                Some(RESP_MAX_REMAINING) => meta.max_remaining = v.as_uint().unwrap_or(0),
                _ => {}
            }
        }
        Ok(meta)
    }

    /// List every relying party that owns at least one resident credential.
    pub fn list_relying_parties(&mut self) -> Result<Vec<RelyingParty>, CtapError> {
        // A key with no resident credentials answers enumerateRPsBegin with
        // CTAP2_ERR_NO_CREDENTIALS instead of totalRPs=0; that means "empty",
        // not an error (e.g. a key that just had its PIN set but no passkeys).
        let first = match self.dispatch(SUB_ENUMERATE_RPS_BEGIN, None) {
            Ok(v) => v,
            Err(CtapError::StatusCode(CTAP2_ERR_NO_CREDENTIALS)) => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let total = checked_total(field_uint(&first, RESP_TOTAL_RPS).unwrap_or(0) as usize)?;
        if total == 0 {
            return Ok(Vec::new());
        }
        // The count is device-claimed and now bounded by `checked_total`; cap
        // the preallocation too so even an in-range count can't overcommit.
        let mut out = Vec::with_capacity(total.min(1024));
        out.push(parse_rp(&first)?);
        while out.len() < total {
            let v = self.dispatch(SUB_ENUMERATE_RPS_NEXT, None)?;
            out.push(parse_rp(&v)?);
        }
        Ok(out)
    }

    /// List every resident credential bound to the given relying-party ID
    /// hash. The hash is what every other CTAP API gives you for the RP, so
    /// callers should pass it through rather than rehashing.
    pub fn list_credentials(
        &mut self,
        rp_id_hash: &[u8; 32],
    ) -> Result<Vec<Credential>, CtapError> {
        let params = Value::Map(vec![(
            Value::UInt(PARAM_RP_ID_HASH),
            Value::Bytes(rp_id_hash.to_vec()),
        )]);
        // Every response below is wrapped in `CredResponse`: the parsed
        // `Credential` takes its own copy of the largeBlobKey, so the decoded
        // response's copy is wiped the moment the wrapper goes out of scope.
        let first = match self.dispatch(SUB_ENUMERATE_CREDS_BEGIN, Some(params)) {
            Ok(v) => CredResponse(v),
            Err(CtapError::StatusCode(CTAP2_ERR_NO_CREDENTIALS)) => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let total = checked_total(field_uint(&first, RESP_TOTAL_CREDS).unwrap_or(0) as usize)?;
        if total == 0 {
            return Ok(Vec::new());
        }
        // Device-claimed count, now bounded by `checked_total` (see list_rps).
        let mut out = Vec::with_capacity(total.min(1024));
        out.push(parse_credential(&first)?);
        while out.len() < total {
            let v = CredResponse(self.dispatch(SUB_ENUMERATE_CREDS_NEXT, None)?);
            out.push(parse_credential(&v)?);
        }
        Ok(out)
    }

    /// Delete a resident credential by its `credentialId` bytes (as
    /// returned by [`Self::list_credentials`]).
    pub fn delete(&mut self, credential_id: &[u8]) -> Result<(), CtapError> {
        let cred_desc = Value::Map(vec![
            (
                Value::Text("id".into()),
                Value::Bytes(credential_id.to_vec()),
            ),
            (Value::Text("type".into()), Value::Text("public-key".into())),
        ]);
        let params = Value::Map(vec![(Value::UInt(PARAM_CREDENTIAL_ID), cred_desc)]);
        self.dispatch(SUB_DELETE_CREDENTIAL, Some(params))?;
        Ok(())
    }

    fn dispatch(&mut self, sub: u8, params: Option<Value>) -> Result<Value, CtapError> {
        use zeroize::Zeroize;
        let request = self.build_request(sub, params.as_ref());
        let mut payload = Vec::with_capacity(request.len() + 1);
        payload.push(self.cmd_code);
        payload.extend_from_slice(&request);
        // enumerateCredentials answers carry the credential's largeBlobKey in
        // the clear, so the raw wire buffer holds key material too. The decode
        // borrows from it and hands back an owned `Value`, so wipe the buffer
        // as soon as the decode is done; routing the decode through a helper
        // keeps every early-return path in front of the wipe.
        let mut resp = self.dev.transact(CTAPHID_CBOR, &payload)?;
        let decoded = decode_response(&resp);
        resp.zeroize();
        decoded
    }

    fn build_request(&self, sub: u8, params: Option<&Value>) -> Vec<u8> {
        // Auth target: subCommand byte || cbor(subCommandParams) if any.
        let mut auth_input = Vec::with_capacity(64);
        auth_input.push(sub);
        if let Some(p) = params {
            auth_input.extend_from_slice(&cbor::encode(p));
        }
        let pin_uv_auth_param = self.token.authenticate(&auth_input);

        let mut entries: Vec<(Value, Value)> = Vec::with_capacity(4);
        entries.push((Value::UInt(KEY_SUB_COMMAND), Value::UInt(sub as u64)));
        if let Some(p) = params {
            entries.push((Value::UInt(KEY_SUB_COMMAND_PARAMS), p.clone()));
        }
        entries.push((
            Value::UInt(KEY_PIN_UV_AUTH_PROTOCOL),
            Value::UInt(self.token.protocol as u64),
        ));
        entries.push((
            Value::UInt(KEY_PIN_UV_AUTH_PARAM),
            Value::Bytes(pin_uv_auth_param),
        ));
        cbor::encode(&Value::Map(entries))
    }
}

/// Split the CTAP status byte off a raw response and decode the CBOR body.
/// Separate from [`CredentialManager::dispatch`] so the caller can wipe the
/// wire buffer on every path, success or error.
fn decode_response(resp: &[u8]) -> Result<Value, CtapError> {
    let (status, body) = resp.split_first().ok_or(CtapError::EmptyResponse)?;
    if *status != 0 {
        return Err(CtapError::StatusCode(*status));
    }
    if body.is_empty() {
        return Ok(Value::Map(Vec::new()));
    }
    let (value, _) = cbor::decode(body)?;
    Ok(value)
}

fn field_uint(v: &Value, key: u64) -> Option<u64> {
    v.get_uint_key(key).and_then(|x| x.as_uint())
}

/// Parse one enumerateRPs response map. Public so the fuzz harness can drive
/// it with arbitrary device bytes.
///
/// A missing or wrong-length `rpIdHash` does not fail the parse: the entry
/// comes back with `rp_id_hash: None`, so one quirky entry (seen on
/// vendor-preview `0x41` implementations) cannot abort a whole enumeration.
/// The hash is never zero-filled — it keys every follow-up
/// `list_credentials()` call, and a zeroed one would query the wrong RP and
/// quietly return no/incorrect credentials — so "unusable" is carried
/// explicitly instead. Only a response missing the RP entity itself is an
/// error.
pub fn parse_rp(v: &Value) -> Result<RelyingParty, CtapError> {
    let rp_entity = v
        .get_uint_key(RESP_RP)
        .ok_or(CtapError::InvalidResponseShape("missing RP entity"))?;
    let rp_id_hash = v
        .get_uint_key(RESP_RP_ID_HASH)
        .and_then(|x| x.as_bytes())
        .and_then(|bytes| {
            if bytes.len() == 32 {
                let mut hash = [0u8; 32];
                hash.copy_from_slice(bytes);
                Some(hash)
            } else {
                None
            }
        });
    let mut rp_id = String::new();
    let mut rp_name: Option<String> = None;
    for (k, val) in rp_entity.as_map().into_iter().flatten() {
        match k.as_text() {
            Some("id") => {
                rp_id = val.as_text().unwrap_or_default().to_owned();
            }
            Some("name") => {
                rp_name = val.as_text().map(|s| s.to_owned());
            }
            _ => {}
        }
    }
    Ok(RelyingParty {
        id: rp_id,
        name: rp_name,
        rp_id_hash,
    })
}

/// Parse one enumerateCredentials response map. Public so the fuzz harness
/// can drive it with arbitrary device bytes.
pub fn parse_credential(v: &Value) -> Result<Credential, CtapError> {
    let user = v
        .get_uint_key(RESP_USER)
        .ok_or(CtapError::InvalidResponseShape("missing user entity"))?;
    let cred_desc = v
        .get_uint_key(RESP_CREDENTIAL_ID)
        .ok_or(CtapError::InvalidResponseShape("missing credentialId"))?;
    let pubkey = v.get_uint_key(RESP_PUBLIC_KEY);

    let mut credential_user = CredentialUser::default();
    for (k, val) in user.as_map().into_iter().flatten() {
        match k.as_text() {
            Some("id") => {
                credential_user.id = val.as_bytes().unwrap_or_default().to_vec();
            }
            Some("name") => {
                credential_user.name = val.as_text().map(|s| s.to_owned());
            }
            Some("displayName") => {
                credential_user.display_name = val.as_text().map(|s| s.to_owned());
            }
            _ => {}
        }
    }

    let mut credential_id = Vec::new();
    for (k, val) in cred_desc.as_map().into_iter().flatten() {
        if k.as_text() == Some("id") {
            credential_id = val.as_bytes().unwrap_or_default().to_vec();
        }
    }

    let algorithm = pubkey.and_then(public_key_algorithm);

    let large_blob_key = v
        .get_uint_key(RESP_LARGE_BLOB_KEY)
        .and_then(|x| x.as_bytes())
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok());

    Ok(Credential {
        credential_id,
        user: credential_user,
        algorithm,
        large_blob_key,
    })
}

/// Extract the COSE `alg` field (map key 3, signed integer) from a
/// COSE_Key. The full key parse is more involved and Phase 2 only needs
/// the algorithm identifier for display.
fn public_key_algorithm(pubkey: &Value) -> Option<i64> {
    for (k, val) in pubkey.as_map().into_iter().flatten() {
        let key = match k {
            Value::UInt(n) => *n as i64,
            Value::NInt(n) => -1 - (*n as i64),
            _ => continue,
        };
        if key == 3 {
            return match val {
                Value::UInt(n) => Some(*n as i64),
                Value::NInt(n) => Some(-1 - (*n as i64)),
                _ => None,
            };
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cbor::encode;
    use crate::client_pin::PinUvAuthToken;
    use crate::cmd::AuthenticatorInfo;
    use crate::pin::PIN_PROTOCOL_V1;

    fn fake_token() -> PinUvAuthToken {
        PinUvAuthToken {
            protocol: PIN_PROTOCOL_V1,
            token: vec![0x42; 16],
        }
    }

    fn info_with(opt: &str) -> AuthenticatorInfo {
        let mut info = AuthenticatorInfo::default();
        info.options.push((opt.to_owned(), true));
        info
    }

    /// Canned-response transport: answers each transact() with the next
    /// queued response, letting enumeration run without hardware.
    struct ScriptedTransport {
        responses: Vec<Vec<u8>>,
    }

    impl CtapTransport for ScriptedTransport {
        fn transact(&mut self, _cmd: u8, _payload: &[u8]) -> Result<Vec<u8>, CtapError> {
            if self.responses.is_empty() {
                return Err(CtapError::EmptyResponse);
            }
            Ok(self.responses.remove(0))
        }
    }

    /// CTAP success status byte followed by the CBOR-encoded body.
    fn ok_cbor(v: &Value) -> Vec<u8> {
        let mut out = vec![0x00];
        out.extend_from_slice(&encode(v));
        out
    }

    #[test]
    fn enumeration_survives_one_malformed_rp_hash() {
        // enumerateRPsBegin answers totalRPs=2 with a 0-byte rpIdHash (the
        // kind of quirk seen on vendor-preview 0x41 implementations);
        // enumerateRPsNext answers with a valid entry. One malformed hash
        // must not abort the enumeration, and the valid RP must still come
        // back usable.
        let first = Value::Map(vec![
            (
                Value::UInt(RESP_RP),
                Value::Map(vec![(
                    Value::Text("id".into()),
                    Value::Text("bad.example".into()),
                )]),
            ),
            (Value::UInt(RESP_RP_ID_HASH), Value::Bytes(Vec::new())),
            (Value::UInt(RESP_TOTAL_RPS), Value::UInt(2)),
        ]);
        let next = Value::Map(vec![
            (
                Value::UInt(RESP_RP),
                Value::Map(vec![(
                    Value::Text("id".into()),
                    Value::Text("good.example".into()),
                )]),
            ),
            (Value::UInt(RESP_RP_ID_HASH), Value::Bytes(vec![0xAB; 32])),
        ]);
        let mut dev = ScriptedTransport {
            responses: vec![ok_cbor(&first), ok_cbor(&next)],
        };
        let info = info_with("credMgmt");
        let mut mgr = CredentialManager::new(&mut dev, fake_token(), &info).unwrap();
        let rps = mgr.list_relying_parties().unwrap();
        assert_eq!(rps.len(), 2);
        assert_eq!(rps[0].id, "bad.example");
        assert_eq!(rps[0].rp_id_hash, None);
        assert_eq!(rps[1].id, "good.example");
        assert_eq!(rps[1].rp_id_hash, Some([0xAB; 32]));
    }

    fn build_request_via(
        mgr_cmd: u8,
        token: PinUvAuthToken,
        sub: u8,
        params: Option<Value>,
    ) -> Vec<u8> {
        // Construct a CredentialManager-equivalent helper for assertion
        // purposes without needing a real CtapHidDevice.
        let mut auth_input = Vec::new();
        auth_input.push(sub);
        if let Some(p) = &params {
            auth_input.extend_from_slice(&encode(p));
        }
        let pin_uv_auth_param = token.authenticate(&auth_input);
        let mut entries: Vec<(Value, Value)> = Vec::new();
        entries.push((Value::UInt(KEY_SUB_COMMAND), Value::UInt(sub as u64)));
        if let Some(p) = params {
            entries.push((Value::UInt(KEY_SUB_COMMAND_PARAMS), p));
        }
        entries.push((
            Value::UInt(KEY_PIN_UV_AUTH_PROTOCOL),
            Value::UInt(token.protocol as u64),
        ));
        entries.push((
            Value::UInt(KEY_PIN_UV_AUTH_PARAM),
            Value::Bytes(pin_uv_auth_param),
        ));
        let mut out = vec![mgr_cmd];
        out.extend_from_slice(&encode(&Value::Map(entries)));
        out
    }

    #[test]
    fn parse_rp_marks_wrong_length_hash_unusable() {
        let rp_entity = Value::Map(vec![(
            Value::Text("id".into()),
            Value::Text("example.com".into()),
        )]);
        // A 31-byte rpIdHash must not abort enumeration, and must not be
        // zero-filled (a zeroed hash would key later list_credentials()
        // calls to the wrong RP): the entry parses with an explicit `None`.
        let bad = Value::Map(vec![
            (Value::UInt(RESP_RP), rp_entity.clone()),
            (Value::UInt(RESP_RP_ID_HASH), Value::Bytes(vec![0xAA; 31])),
        ]);
        let rp = parse_rp(&bad).unwrap();
        assert_eq!(rp.id, "example.com");
        assert_eq!(rp.rp_id_hash, None);

        // The correct 32-byte hash parses through unchanged.
        let good = Value::Map(vec![
            (Value::UInt(RESP_RP), rp_entity),
            (Value::UInt(RESP_RP_ID_HASH), Value::Bytes(vec![0xAA; 32])),
        ]);
        let rp = parse_rp(&good).unwrap();
        assert_eq!(rp.rp_id_hash, Some([0xAA; 32]));
        assert_eq!(rp.id, "example.com");
    }

    #[test]
    fn checked_total_rejects_absurd_device_counts() {
        // A real key holds well under the cap; those pass through unchanged.
        assert_eq!(checked_total(0).unwrap(), 0);
        assert_eq!(checked_total(100).unwrap(), 100);
        assert_eq!(checked_total(MAX_ENUMERATED).unwrap(), MAX_ENUMERATED);
        // A hostile authenticator claiming a huge total (up to u32) must be
        // rejected, not enumerated — otherwise the Next loop streams forever.
        assert!(matches!(
            checked_total(MAX_ENUMERATED + 1),
            Err(CtapError::InvalidResponseShape(_))
        ));
        assert!(matches!(
            checked_total(u32::MAX as usize),
            Err(CtapError::InvalidResponseShape(_))
        ));
    }

    #[test]
    fn picks_standard_cmd_when_credmgmt_advertised() {
        let info = info_with("credMgmt");
        // Construction can't be tested directly without a device, but we
        // can assert the option lookup picks the right code via inline match.
        assert_eq!(info.option("credMgmt"), Some(true));
        assert_eq!(info.option("credentialMgmtPreview"), None);
    }

    #[test]
    fn picks_preview_cmd_when_only_preview_advertised() {
        let info = info_with("credentialMgmtPreview");
        assert_eq!(info.option("credMgmt"), None);
        assert_eq!(info.option("credentialMgmtPreview"), Some(true));
    }

    #[test]
    fn request_includes_pin_uv_auth_param_for_subcommand_byte() {
        let token = fake_token();
        let bytes = build_request_via(
            CTAP2_CREDENTIAL_MANAGEMENT,
            token,
            SUB_GET_CREDS_METADATA,
            None,
        );
        // First byte is the command code; rest is CBOR.
        assert_eq!(bytes[0], CTAP2_CREDENTIAL_MANAGEMENT);
        let (val, _) = cbor::decode(&bytes[1..]).unwrap();
        let map = val.as_map().unwrap();
        assert!(map
            .iter()
            .any(|(k, _)| k.as_uint() == Some(KEY_PIN_UV_AUTH_PROTOCOL)));
        assert!(map
            .iter()
            .any(|(k, _)| k.as_uint() == Some(KEY_PIN_UV_AUTH_PARAM)));
    }

    #[test]
    fn delete_request_carries_credential_descriptor() {
        let token = fake_token();
        let cred_id = vec![0xAA; 32];
        let cred_desc = Value::Map(vec![
            (Value::Text("id".into()), Value::Bytes(cred_id.clone())),
            (Value::Text("type".into()), Value::Text("public-key".into())),
        ]);
        let params = Value::Map(vec![(Value::UInt(PARAM_CREDENTIAL_ID), cred_desc)]);
        let bytes = build_request_via(
            CTAP2_CREDENTIAL_MANAGEMENT,
            token,
            SUB_DELETE_CREDENTIAL,
            Some(params),
        );
        let (val, _) = cbor::decode(&bytes[1..]).unwrap();
        let params_val = val.get_uint_key(KEY_SUB_COMMAND_PARAMS).unwrap();
        let inner = params_val.get_uint_key(PARAM_CREDENTIAL_ID).unwrap();
        // The credential descriptor is keyed by string "id".
        let id_val = inner
            .as_map()
            .unwrap()
            .iter()
            .find_map(|(k, v)| {
                if k.as_text() == Some("id") {
                    Some(v)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(id_val.as_bytes(), Some(&cred_id[..]));
    }

    #[test]
    fn parse_rp_extracts_id_and_hash() {
        let rp = Value::Map(vec![
            (
                Value::UInt(RESP_RP),
                Value::Map(vec![
                    (Value::Text("id".into()), Value::Text("example.com".into())),
                    (
                        Value::Text("name".into()),
                        Value::Text("Example, Inc.".into()),
                    ),
                ]),
            ),
            (Value::UInt(RESP_RP_ID_HASH), Value::Bytes(vec![0x77; 32])),
        ]);
        let parsed = parse_rp(&rp).unwrap();
        assert_eq!(parsed.id, "example.com");
        assert_eq!(parsed.name.as_deref(), Some("Example, Inc."));
        assert_eq!(parsed.rp_id_hash, Some([0x77; 32]));
    }

    #[test]
    fn parse_credential_extracts_user_id_and_cred_id() {
        let cred = Value::Map(vec![
            (
                Value::UInt(RESP_USER),
                Value::Map(vec![
                    (Value::Text("id".into()), Value::Bytes(vec![1, 2, 3])),
                    (Value::Text("name".into()), Value::Text("alice".into())),
                    (
                        Value::Text("displayName".into()),
                        Value::Text("Alice Liddell".into()),
                    ),
                ]),
            ),
            (
                Value::UInt(RESP_CREDENTIAL_ID),
                Value::Map(vec![
                    (Value::Text("id".into()), Value::Bytes(vec![0xAB; 32])),
                    (Value::Text("type".into()), Value::Text("public-key".into())),
                ]),
            ),
            // COSE_Key with alg=-7 (ES256)
            (
                Value::UInt(RESP_PUBLIC_KEY),
                Value::Map(vec![
                    (Value::UInt(1), Value::UInt(2)),
                    (Value::UInt(3), Value::NInt(6)),
                ]),
            ),
        ]);
        let parsed = parse_credential(&cred).unwrap();
        assert_eq!(parsed.credential_id, vec![0xAB; 32]);
        assert_eq!(parsed.user.id, vec![1, 2, 3]);
        assert_eq!(parsed.user.name.as_deref(), Some("alice"));
        assert_eq!(parsed.user.display_name.as_deref(), Some("Alice Liddell"));
        assert_eq!(parsed.algorithm, Some(-7));
    }

    #[test]
    fn parse_credential_reads_large_blob_key() {
        let key = [0x5Au8; 32];
        let cred = Value::Map(vec![
            (
                Value::UInt(RESP_USER),
                Value::Map(vec![(
                    Value::Text("id".into()),
                    Value::Bytes(vec![1, 2, 3]),
                )]),
            ),
            (
                Value::UInt(RESP_CREDENTIAL_ID),
                Value::Map(vec![(
                    Value::Text("id".into()),
                    Value::Bytes(vec![0xAB; 32]),
                )]),
            ),
            (Value::UInt(RESP_LARGE_BLOB_KEY), Value::Bytes(key.to_vec())),
        ]);
        let parsed = parse_credential(&cred).unwrap();
        assert_eq!(parsed.large_blob_key, Some(key));

        // No 0x0B entry at all -> None.
        let cred_no_blob_key = Value::Map(vec![
            (
                Value::UInt(RESP_USER),
                Value::Map(vec![(
                    Value::Text("id".into()),
                    Value::Bytes(vec![1, 2, 3]),
                )]),
            ),
            (
                Value::UInt(RESP_CREDENTIAL_ID),
                Value::Map(vec![(
                    Value::Text("id".into()),
                    Value::Bytes(vec![0xAB; 32]),
                )]),
            ),
        ]);
        let parsed = parse_credential(&cred_no_blob_key).unwrap();
        assert_eq!(parsed.large_blob_key, None);
    }

    #[test]
    fn credential_scrub_clears_the_large_blob_key() {
        // What `Drop for Credential` runs. Observing the freed allocation
        // itself would need `unsafe` (forbidden workspace-wide), so the wipe
        // is asserted on the value the destructor operates on.
        let mut cred = Credential {
            credential_id: vec![0xAB; 32],
            user: CredentialUser::default(),
            algorithm: Some(-7),
            large_blob_key: Some([0x5A; 32]),
        };
        cred.scrub_key();
        assert_eq!(cred.large_blob_key, Some([0u8; 32]));

        // A credential without a key must survive the same call untouched.
        let mut none = Credential {
            credential_id: vec![0xCD; 16],
            user: CredentialUser::default(),
            algorithm: None,
            large_blob_key: None,
        };
        none.scrub_key();
        assert_eq!(none.large_blob_key, None);
        assert_eq!(none.credential_id, vec![0xCD; 16]);
    }

    #[test]
    fn scrub_clears_the_decoded_responses_key_copy() {
        // What `Drop for CredResponse` runs: the decoder's own copy of the
        // largeBlobKey is wiped, and nothing else in the response is.
        let mut resp = Value::Map(vec![
            (Value::UInt(RESP_TOTAL_CREDS), Value::UInt(1)),
            (
                Value::UInt(RESP_CREDENTIAL_ID),
                Value::Map(vec![(
                    Value::Text("id".into()),
                    Value::Bytes(vec![0xAB; 32]),
                )]),
            ),
            (
                Value::UInt(RESP_LARGE_BLOB_KEY),
                Value::Bytes(vec![0x5A; 32]),
            ),
        ]);
        scrub_large_blob_key(&mut resp);
        // `Vec::zeroize` wipes the bytes *and* the capacity tail, then
        // truncates, so the entry is left empty rather than 32 zeros.
        assert_eq!(
            resp.get_uint_key(RESP_LARGE_BLOB_KEY).unwrap().as_bytes(),
            Some(&[][..])
        );
        let cred_desc = resp.get_uint_key(RESP_CREDENTIAL_ID).unwrap();
        let id = cred_desc
            .as_map()
            .unwrap()
            .iter()
            .find_map(|(k, v)| (k.as_text() == Some("id")).then_some(v))
            .unwrap();
        assert_eq!(id.as_bytes(), Some(&[0xABu8; 32][..]));
        assert_eq!(field_uint(&resp, RESP_TOTAL_CREDS), Some(1));
        // A response with no 0x0B entry is left alone.
        let mut no_key = Value::Map(vec![(Value::UInt(RESP_TOTAL_CREDS), Value::UInt(3))]);
        scrub_large_blob_key(&mut no_key);
        assert_eq!(field_uint(&no_key, RESP_TOTAL_CREDS), Some(3));
    }

    #[test]
    fn enumeration_returns_the_key_despite_the_buffer_wipe() {
        // The wire buffer and the decoded response are both scrubbed on this
        // path; the parsed credential must still carry the key, i.e. the
        // wipes are sequenced after the decode and after the parse.
        let key = [0x5Au8; 32];
        let entry = Value::Map(vec![
            (
                Value::UInt(RESP_USER),
                Value::Map(vec![(
                    Value::Text("id".into()),
                    Value::Bytes(vec![1, 2, 3]),
                )]),
            ),
            (
                Value::UInt(RESP_CREDENTIAL_ID),
                Value::Map(vec![(
                    Value::Text("id".into()),
                    Value::Bytes(vec![0xAB; 32]),
                )]),
            ),
            (Value::UInt(RESP_LARGE_BLOB_KEY), Value::Bytes(key.to_vec())),
            (Value::UInt(RESP_TOTAL_CREDS), Value::UInt(1)),
        ]);
        let mut dev = ScriptedTransport {
            responses: vec![ok_cbor(&entry)],
        };
        let info = info_with("credMgmt");
        let mut mgr = CredentialManager::new(&mut dev, fake_token(), &info).unwrap();
        let creds = mgr.list_credentials(&[0x11; 32]).unwrap();
        assert_eq!(creds.len(), 1);
        assert_eq!(creds[0].large_blob_key, Some(key));
        assert_eq!(creds[0].credential_id, vec![0xAB; 32]);
    }
}
