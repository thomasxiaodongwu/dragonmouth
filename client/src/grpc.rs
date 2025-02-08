pub mod gen {
    include!(concat!(env!("OUT_DIR"), "/geyser.Geyser.rs"));
}

use {
    crate::{error::ReceiveError, stream::SubscribeStream},
    bytes::{Buf, Bytes},
    futures::{
        channel::mpsc,
        sink::{Sink, SinkExt},
        stream::{Stream, StreamExt},
    },
    gen::geyser_client::GeyserClient,
    pin_project_lite::pin_project,
    prost::Message,
    richat_proto::{
        geyser::{
            CommitmentLevel, GetBlockHeightRequest, GetBlockHeightResponse,
            GetLatestBlockhashRequest, GetLatestBlockhashResponse, GetSlotRequest, GetSlotResponse,
            GetVersionRequest, GetVersionResponse, IsBlockhashValidRequest,
            IsBlockhashValidResponse, PingRequest, PongResponse, SubscribeRequest,
        },
        richat::GrpcSubscribeRequest,
    },
    richat_shared::{
        config::{deserialize_maybe_x_token, deserialize_num_str},
        transports::grpc::{ConfigGrpcCompression, ConfigGrpcServer},
    },
    serde::Deserialize,
    std::{
        collections::HashMap,
        fmt, io,
        marker::PhantomData,
        path::PathBuf,
        pin::Pin,
        task::{Context, Poll},
        time::Duration,
    },
    thiserror::Error,
    tokio::fs,
    tonic::{
        codec::{Codec, CompressionEncoding, DecodeBuf, Decoder, EncodeBuf, Encoder},
        metadata::{errors::InvalidMetadataValueBytes, AsciiMetadataKey, AsciiMetadataValue},
        service::{interceptor::InterceptedService, Interceptor},
        transport::{
            channel::{Channel, ClientTlsConfig, Endpoint},
            Certificate,
        },
        Request, Response, Status, Streaming,
    },
};

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ConfigGrpcClient {
    pub endpoint: String,
    pub ca_certificate: Option<PathBuf>,
    #[serde(with = "humantime_serde")]
    pub connect_timeout: Option<Duration>,
    pub buffer_size: Option<usize>,
    pub http2_adaptive_window: Option<bool>,
    #[serde(with = "humantime_serde")]
    pub http2_keep_alive_interval: Option<Duration>,
    pub initial_connection_window_size: Option<u32>,
    pub initial_stream_window_size: Option<u32>,
    #[serde(with = "humantime_serde")]
    pub keep_alive_timeout: Option<Duration>,
    pub keep_alive_while_idle: bool,
    #[serde(with = "humantime_serde")]
    pub tcp_keepalive: Option<Duration>,
    pub tcp_nodelay: bool,
    #[serde(with = "humantime_serde")]
    pub timeout: Option<Duration>,
    #[serde(deserialize_with = "deserialize_num_str")]
    pub max_decoding_message_size: usize,
    pub compression: ConfigGrpcCompression,
    #[serde(deserialize_with = "deserialize_maybe_x_token")]
    pub x_token: Option<Vec<u8>>,
}

impl Default for ConfigGrpcClient {
    fn default() -> Self {
        Self {
            endpoint: format!("http://{}", ConfigGrpcServer::default().endpoint),
            ca_certificate: None,
            connect_timeout: None,
            buffer_size: None,
            http2_adaptive_window: None,
            http2_keep_alive_interval: None,
            initial_connection_window_size: None,
            initial_stream_window_size: None,
            keep_alive_timeout: None,
            keep_alive_while_idle: false,
            tcp_keepalive: Some(Duration::from_secs(15)),
            tcp_nodelay: true,
            timeout: None,
            max_decoding_message_size: 4 * 1024 * 1024, // 4MiB
            compression: ConfigGrpcCompression::default(),
            x_token: None,
        }
    }
}

impl ConfigGrpcClient {
    pub async fn connect(self) -> Result<GrpcClient<impl Interceptor>, GrpcClientBuilderError> {
        let mut builder = GrpcClientBuilder::from_shared(self.endpoint)?
            .tls_config_native_roots(self.ca_certificate.as_ref())
            .await?
            .buffer_size(self.buffer_size)
            .keep_alive_while_idle(self.keep_alive_while_idle)
            .tcp_keepalive(self.tcp_keepalive)
            .tcp_nodelay(self.tcp_nodelay)
            .max_decoding_message_size(self.max_decoding_message_size)
            .x_token(self.x_token)?;
        if let Some(connect_timeout) = self.connect_timeout {
            builder = builder.connect_timeout(connect_timeout)
        }
        if let Some(http2_adaptive_window) = self.http2_adaptive_window {
            builder = builder.http2_adaptive_window(http2_adaptive_window);
        }
        if let Some(http2_keep_alive_interval) = self.http2_keep_alive_interval {
            builder = builder.http2_keep_alive_interval(http2_keep_alive_interval);
        }
        if let Some(initial_connection_window_size) = self.initial_connection_window_size {
            builder = builder.initial_connection_window_size(initial_connection_window_size);
        }
        if let Some(initial_stream_window_size) = self.initial_stream_window_size {
            builder = builder.initial_stream_window_size(initial_stream_window_size);
        }
        if let Some(keep_alive_timeout) = self.keep_alive_timeout {
            builder = builder.keep_alive_timeout(keep_alive_timeout);
        }
        if let Some(timeout) = self.timeout {
            builder = builder.timeout(timeout);
        }
        for encoding in self.compression.accept {
            builder = builder.accept_compressed(encoding);
        }
        for encoding in self.compression.send {
            builder = builder.send_compressed(encoding);
        }
        builder.connect().await.map_err(Into::into)
    }
}

#[derive(Debug, Error)]
pub enum GrpcClientBuilderError {
    #[error("failed to load cert: {0}")]
    LoadCert(io::Error),
    #[error("tonic error: {0}")]
    Tonic(#[from] tonic::transport::Error),
    #[error("x-token error: {0}")]
    XToken(#[from] InvalidMetadataValueBytes),
}

#[derive(Debug)]
pub struct GrpcClientBuilder {
    pub endpoint: Endpoint,
    pub send_compressed: Option<CompressionEncoding>,
    pub accept_compressed: Option<CompressionEncoding>,
    pub max_decoding_message_size: Option<usize>,
    pub max_encoding_message_size: Option<usize>,
    pub interceptor: GrpcInterceptor,
}

impl GrpcClientBuilder {
    // Create new builder
    fn new(endpoint: Endpoint) -> Self {
        Self {
            endpoint,
            send_compressed: None,
            accept_compressed: None,
            max_decoding_message_size: None,
            max_encoding_message_size: None,
            interceptor: GrpcInterceptor::default(),
        }
    }

    pub fn from_shared(endpoint: impl Into<Bytes>) -> Result<Self, tonic::transport::Error> {
        Endpoint::from_shared(endpoint).map(Self::new)
    }

    pub fn from_static(endpoint: &'static str) -> Self {
        Self::new(Endpoint::from_static(endpoint))
    }

    // Endpoint options
    pub fn connect_timeout(self, dur: Duration) -> Self {
        Self {
            endpoint: self.endpoint.connect_timeout(dur),
            ..self
        }
    }

    pub fn buffer_size(self, sz: impl Into<Option<usize>>) -> Self {
        Self {
            endpoint: self.endpoint.buffer_size(sz),
            ..self
        }
    }

    pub fn http2_adaptive_window(self, enabled: bool) -> Self {
        Self {
            endpoint: self.endpoint.http2_adaptive_window(enabled),
            ..self
        }
    }

    pub fn http2_keep_alive_interval(self, interval: Duration) -> Self {
        Self {
            endpoint: self.endpoint.http2_keep_alive_interval(interval),
            ..self
        }
    }

    pub fn initial_connection_window_size(self, sz: impl Into<Option<u32>>) -> Self {
        Self {
            endpoint: self.endpoint.initial_connection_window_size(sz),
            ..self
        }
    }

    pub fn initial_stream_window_size(self, sz: impl Into<Option<u32>>) -> Self {
        Self {
            endpoint: self.endpoint.initial_stream_window_size(sz),
            ..self
        }
    }

    pub fn keep_alive_timeout(self, duration: Duration) -> Self {
        Self {
            endpoint: self.endpoint.keep_alive_timeout(duration),
            ..self
        }
    }

    pub fn keep_alive_while_idle(self, enabled: bool) -> Self {
        Self {
            endpoint: self.endpoint.keep_alive_while_idle(enabled),
            ..self
        }
    }

    pub fn tcp_keepalive(self, tcp_keepalive: Option<Duration>) -> Self {
        Self {
            endpoint: self.endpoint.tcp_keepalive(tcp_keepalive),
            ..self
        }
    }

    pub fn tcp_nodelay(self, enabled: bool) -> Self {
        Self {
            endpoint: self.endpoint.tcp_nodelay(enabled),
            ..self
        }
    }

    pub fn timeout(self, dur: Duration) -> Self {
        Self {
            endpoint: self.endpoint.timeout(dur),
            ..self
        }
    }

    pub fn tls_config(self, tls_config: ClientTlsConfig) -> Result<Self, GrpcClientBuilderError> {
        Ok(Self {
            endpoint: self.endpoint.tls_config(tls_config)?,
            ..self
        })
    }

    pub async fn tls_config_native_roots(
        self,
        ca_certificate: Option<&PathBuf>,
    ) -> Result<Self, GrpcClientBuilderError> {
        let mut tls_config = ClientTlsConfig::new().with_native_roots();
        if let Some(path) = ca_certificate {
            let bytes = fs::read(path)
                .await
                .map_err(GrpcClientBuilderError::LoadCert)?;
            tls_config = tls_config.ca_certificate(Certificate::from_pem(bytes));
        }
        self.tls_config(tls_config)
    }

    // gRPC options
    pub fn send_compressed(self, encoding: CompressionEncoding) -> Self {
        Self {
            send_compressed: Some(encoding),
            ..self
        }
    }

    pub fn accept_compressed(self, encoding: CompressionEncoding) -> Self {
        Self {
            accept_compressed: Some(encoding),
            ..self
        }
    }

    pub fn max_decoding_message_size(self, limit: usize) -> Self {
        Self {
            max_decoding_message_size: Some(limit),
            ..self
        }
    }

    pub fn max_encoding_message_size(self, limit: usize) -> Self {
        Self {
            max_encoding_message_size: Some(limit),
            ..self
        }
    }

    // Metadata
    pub fn x_token<T>(mut self, x_token: Option<T>) -> Result<Self, InvalidMetadataValueBytes>
    where
        T: TryInto<AsciiMetadataValue, Error = InvalidMetadataValueBytes>,
    {
        if let Some(x_token) = x_token {
            self.interceptor.metadata.insert(
                AsciiMetadataKey::from_static("x-token"),
                x_token.try_into()?,
            );
        } else {
            self.interceptor.metadata.remove("x-token");
        }
        Ok(self)
    }

    // Create client
    fn build(self, channel: Channel) -> GrpcClient<impl Interceptor> {
        let mut geyser = GeyserClient::with_interceptor(channel, self.interceptor);
        if let Some(encoding) = self.send_compressed {
            geyser = geyser.send_compressed(encoding);
        }
        if let Some(encoding) = self.accept_compressed {
            geyser = geyser.accept_compressed(encoding);
        }
        if let Some(limit) = self.max_decoding_message_size {
            geyser = geyser.max_decoding_message_size(limit);
        }
        if let Some(limit) = self.max_encoding_message_size {
            geyser = geyser.max_encoding_message_size(limit);
        }
        GrpcClient::new(geyser)
    }

    pub async fn connect(self) -> Result<GrpcClient<impl Interceptor>, tonic::transport::Error> {
        let channel = self.endpoint.connect().await?;
        Ok(self.build(channel))
    }

    pub fn connect_lazy(self) -> Result<GrpcClient<impl Interceptor>, tonic::transport::Error> {
        let channel = self.endpoint.connect_lazy();
        Ok(self.build(channel))
    }
}

#[derive(Debug, Default)]
pub struct GrpcInterceptor {
    metadata: HashMap<AsciiMetadataKey, AsciiMetadataValue>,
}

impl Interceptor for GrpcInterceptor {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        for (key, value) in self.metadata.iter() {
            request.metadata_mut().insert(key, value.clone());
        }
        Ok(request)
    }
}

#[derive(Debug)]
pub struct GrpcClient<F> {
    geyser: GeyserClient<InterceptedService<Channel, F>>,
}

impl GrpcClient<()> {
    pub fn build_from_shared(
        endpoint: impl Into<Bytes>,
    ) -> Result<GrpcClientBuilder, tonic::transport::Error> {
        Ok(GrpcClientBuilder::new(Endpoint::from_shared(endpoint)?))
    }

    pub fn build_from_static(endpoint: &'static str) -> GrpcClientBuilder {
        GrpcClientBuilder::new(Endpoint::from_static(endpoint))
    }
}

impl<F: Interceptor> GrpcClient<F> {
    pub const fn new(geyser: GeyserClient<InterceptedService<Channel, F>>) -> Self {
        Self { geyser }
    }

    // Subscribe Yellowstone gRPC Dragon's Mouth
    pub async fn subscribe_dragons_mouth(
        &mut self,
    ) -> Result<
        (
            impl Sink<SubscribeRequest, Error = mpsc::SendError>,
            GrpcClientStream,
        ),
        Status,
    > {
        let (subscribe_tx, subscribe_rx) = mpsc::unbounded();
        let response: Response<Streaming<Vec<u8>>> = self.geyser.subscribe(subscribe_rx).await?;
        let stream = GrpcClientStream {
            stream: response.into_inner(),
        };
        Ok((subscribe_tx, stream))
    }

    pub async fn subscribe_dragons_mouth_once(
        &mut self,
        request: SubscribeRequest,
    ) -> Result<GrpcClientStream, Status> {
        let (mut tx, rx) = self.subscribe_dragons_mouth().await?;
        tx.send(request)
            .await
            .expect("failed to send to unbounded channel");
        Ok(rx)
    }

    // Subscribe Richat
    pub async fn subscribe_richat(
        &mut self,
        request: GrpcSubscribeRequest,
    ) -> Result<GrpcClientStream, Status> {
        let (mut tx, rx) = mpsc::unbounded();
        tx.send(request)
            .await
            .expect("failed to send to unbounded channel");

        let response: Response<Streaming<Vec<u8>>> = self.geyser.subscribe_richat(rx).await?;
        let stream = response.into_inner();
        Ok(GrpcClientStream { stream })
    }

    // RPC calls
    pub async fn ping(&mut self, count: i32) -> Result<PongResponse, Status> {
        let message = PingRequest { count };
        let request = Request::new(message);
        let response = self.geyser.ping(request).await?;
        Ok(response.into_inner())
    }

    pub async fn get_latest_blockhash(
        &mut self,
        commitment: Option<CommitmentLevel>,
    ) -> Result<GetLatestBlockhashResponse, Status> {
        let request = Request::new(GetLatestBlockhashRequest {
            commitment: commitment.map(|value| value as i32),
        });
        let response = self.geyser.get_latest_blockhash(request).await?;
        Ok(response.into_inner())
    }

    pub async fn get_block_height(
        &mut self,
        commitment: Option<CommitmentLevel>,
    ) -> Result<GetBlockHeightResponse, Status> {
        let request = Request::new(GetBlockHeightRequest {
            commitment: commitment.map(|value| value as i32),
        });
        let response = self.geyser.get_block_height(request).await?;
        Ok(response.into_inner())
    }

    pub async fn get_slot(
        &mut self,
        commitment: Option<CommitmentLevel>,
    ) -> Result<GetSlotResponse, Status> {
        let request = Request::new(GetSlotRequest {
            commitment: commitment.map(|value| value as i32),
        });
        let response = self.geyser.get_slot(request).await?;
        Ok(response.into_inner())
    }

    pub async fn is_blockhash_valid(
        &mut self,
        blockhash: String,
        commitment: Option<CommitmentLevel>,
    ) -> Result<IsBlockhashValidResponse, Status> {
        let request = Request::new(IsBlockhashValidRequest {
            blockhash,
            commitment: commitment.map(|value| value as i32),
        });
        let response = self.geyser.is_blockhash_valid(request).await?;
        Ok(response.into_inner())
    }

    pub async fn get_version(&mut self) -> Result<GetVersionResponse, Status> {
        let request = Request::new(GetVersionRequest {});
        let response = self.geyser.get_version(request).await?;
        Ok(response.into_inner())
    }
}

trait SubscribeMessage {
    fn decode(buf: &mut DecodeBuf<'_>) -> Self;
}

impl SubscribeMessage for Vec<u8> {
    fn decode(src: &mut DecodeBuf<'_>) -> Self {
        // TODO: use Box<[MaybeUninit<u8>]> (from rust 1.82.0)
        let mut dst = Vec::with_capacity(src.remaining());
        #[allow(clippy::uninit_vec)]
        unsafe {
            dst.set_len(src.remaining());
        }
        let mut start = 0;
        while src.remaining() > 0 {
            let chunk = src.chunk();
            dst.as_mut_slice()[start..start + chunk.len()].copy_from_slice(chunk);
            start += chunk.len();
            src.advance(chunk.len());
        }
        dst
    }
}

pub struct SubscribeCodec<T, U> {
    _pd: PhantomData<(T, U)>,
}

impl<T, U> Default for SubscribeCodec<T, U> {
    fn default() -> Self {
        Self { _pd: PhantomData }
    }
}

impl<T, U> Codec for SubscribeCodec<T, U>
where
    T: Message + Send + 'static,
    U: SubscribeMessage + Default + Send + 'static,
{
    type Encode = T;
    type Decode = U;

    type Encoder = ProstEncoder<T>;
    type Decoder = SubscribeDecoder<U>;

    fn encoder(&mut self) -> Self::Encoder {
        ProstEncoder(PhantomData)
    }

    fn decoder(&mut self) -> Self::Decoder {
        SubscribeDecoder(PhantomData)
    }
}

/// A [`Encoder`] that knows how to encode `T`.
#[derive(Debug, Clone, Default)]
pub struct ProstEncoder<T>(PhantomData<T>);

impl<T: Message> Encoder for ProstEncoder<T> {
    type Item = T;
    type Error = Status;

    fn encode(&mut self, item: Self::Item, buf: &mut EncodeBuf<'_>) -> Result<(), Self::Error> {
        item.encode(buf)
            .expect("Message only errors if not enough space");
        Ok(())
    }
}

/// A [`Decoder`] that knows how to decode `U`.
#[derive(Debug, Clone, Default)]
pub struct SubscribeDecoder<U>(PhantomData<U>);

impl<U: SubscribeMessage + Default> Decoder for SubscribeDecoder<U> {
    type Item = U;
    type Error = Status;

    fn decode(&mut self, buf: &mut DecodeBuf<'_>) -> Result<Option<Self::Item>, Self::Error> {
        Ok(Some(SubscribeMessage::decode(buf)))
    }
}

pin_project! {
    pub struct GrpcClientStream {
        #[pin]
        stream: Streaming<Vec<u8>>,
    }
}

impl GrpcClientStream {
    pub fn into_parsed(self) -> SubscribeStream {
        SubscribeStream::new(self.boxed())
    }
}

impl fmt::Debug for GrpcClientStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GrpcClientStream").finish()
    }
}

impl Stream for GrpcClientStream {
    type Item = Result<Vec<u8>, ReceiveError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let me = self.project();
        me.stream.poll_next(cx).map_err(Into::into)
    }
}
