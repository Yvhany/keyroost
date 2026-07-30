# Code Signing Certificate

## 证书信息
- **名称**: yvhan_dev_RSA
- **算法**: RSA 2048-bit
- **指纹**: 61F4C2C078253BC03318D6536F053B7FB5685604
- **有效期至**: 2029-07-31
- **颁发者**: 自签名

## 文件说明
- yvhan_dev_RSA.cer - 公钥证书（用于验证签名）
- yvhan_dev_RSA.pfx - 私钥证书（仅本地保存，勿上传）

## 验证签名
使用以下命令验证可执行文件签名：
`powershell
signtool verify /pa keyroost-v0.7.6-zh-CN_x64.exe
signtool verify /pa keyroost-v0.7.6-zh-CN_x86.exe
`

## 注意事项
- 此证书为自签名证书，Windows 会显示"未知发布者"警告
- 公开分发请考虑购买商业代码签名证书