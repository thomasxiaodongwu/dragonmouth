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

    配置文件：去掉grpc转接，把quic加上
    代码：注释掉richat中parser_jh与apps_jh线程，作为测试做到仅仅是接收消息而不转发，

## 客户端与服务端集成测试
    
    进行中

# Test Environment
Memory 64GB
CPU Intel Core i9-10885H CPU @ 2.40GHz, Eight Cores
Operating System Ubuntu 22.04.2 LTS
Solana Validator 2.1.13

## Geyser Modification Details
config-test.json

    Removed grpc and ws configurations as feedback indicated only quic supports new feature

    If grpc configuration is added, crt and key need to be included, and recompiled
plugin.rs

    Modified as follows, as there were some data version mismatches in testvalidator resulting in errors upon startup, but unsure if this will occur in production

    ReplicaBlockInfoVersions::V0_0_4(info) => {
    let inner = self.inner.as_ref().expect("initialized");
    inner
    .messages
    .push(ProtobufMessage::BlockMeta { blockinfo: info }, inner.encoder);
    }
    _ => {
    // Ignoring other versions
    warn!("Ignoring unsupported ReplicaBlockInfoVersions");
    }

## Startup Procedure
    First compile cargo b --release to generate the .so file in the outer layer, size is 1GB
    Then start solana-test-validator --geyser-plugin-config plugin-agave/config-test.json
    Log will indicate [richat_shared::transports::quic] start server at 127.0.0.1:10100

## Client
    Configuration file: Removed grpc forwarding, added quic
    Code: Commented out richat's parser_jh and apps_jh threads for testing to only receive messages without forwarding

## Client-Server Integration Testing
    In progress