use rdkafka::config::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::error::KafkaError;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

/// KafkaProducer：封装 Kafka 生产者逻辑，提供初始化和消息发送的接口
pub struct KafkaProducer {
    producer: Arc<FutureProducer>, // 使用 Arc 以便多线程共享
    topic: String,                 // 默认主题
}

impl KafkaProducer {
    /// 创建一个新的 KafkaProducer 实例
    pub fn new(brokers: &str, topic: &str) -> Self {
        let producer = ClientConfig::new()
            .set("bootstrap.servers", brokers)
            .set("message.timeout.ms", "5000") // 消息超时时间
            .create()
            .expect("Failed to create Kafka producer");

        KafkaProducer {
            producer: Arc::new(producer),
            topic: topic.to_string(),
        }
    }

    /// 向 Kafka 发送消息，带自动重试机制
    pub async fn send_message<P>(&self, key: &str, payload: P)
    where
        P: AsRef<[u8]>, // 泛型约束，支持任何可以引用为字节数组的类型
    {
        loop {
            let record = FutureRecord::to(&self.topic)
                .payload(payload.as_ref()) // 将 payload 转为字节数组
                .key(key);

            match self.producer.send(record, Duration::from_secs(0)).await {
                Ok(delivery) => {
                    println!("Message delivered: {:?}", delivery);
                    break;
                }
                Err((KafkaError::MessageProduction(err), _)) => {
                    eprintln!("Message production error: {:?}", err);
                    sleep(Duration::from_secs(1)).await; // 重试前等待
                }
                Err((err, _)) => {
                    eprintln!("Failed to deliver message: {:?}", err);
                    sleep(Duration::from_secs(1)).await; // 重试前等待
                }
            }
        }
    }

    /// 获取生产者对象（共享引用）
    pub fn get_producer(&self) -> Arc<FutureProducer> {
        Arc::clone(&self.producer)
    }
}