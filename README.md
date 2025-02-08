# 测试环境
内存 64g
cpu 英特尔 Core i9-10885H CPU @ 2.40GHz 八核
系统 Ubuntu 22.04.2 LTS
solana validator 2.1.13

## geyser修改细节
config-test.json

    去掉grpc与ws的配置，因为根据反馈只有quic支持newfeature

    如果加上grpc配置的话需要加上crt与key，重新编译
plugin.rs

    修改如下，因为是testvalidator数据版本有些对不上启动报错，但是不确定正式的是否会报错

    ReplicaBlockInfoVersions::V0_0_4(info) => {
    let inner = self.inner.as_ref().expect("initialized");
    inner
    .messages
    .push(ProtobufMessage::BlockMeta { blockinfo: info }, inner.encoder);
    }
    _ => {
    // 忽略其他版本
    warn!("Ignoring unsupported ReplicaBlockInfoVersions");
    }

## 启动方式
    先在外层编译cargo b --release生成so文件，大小为1GB
    然后在solana-test-validator --geyser-plugin-config plugin-agave/config-test.json启动
    启动日志中提示richat_shared::transports::quic] start server at 127.0.0.1:10100

## client客户端

    开发中
