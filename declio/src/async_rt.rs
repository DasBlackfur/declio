use tokio::io;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use std::borrow::Cow;
use std::mem;

use crate::ctx::Len;
use crate::{Endian, Error};


/// A type that can be encoded into a byte stream.
pub trait AsyncEncode<Ctx = ()> {
    /// Encodes `&self` to the given writer.
    fn encode<W>(&self, ctx: Ctx, writer: &mut W) -> impl std::future::Future<Output = Result<(), Error>> + Send
    where
        W: io::AsyncWrite + Unpin + Send;
}

/// A type that can be decoded from a byte stream.
pub trait AsyncDecode<Ctx = ()>: Sized {
    /// Decodes a value from the given reader.
    async fn decode<R>(ctx: Ctx, reader: &mut R) -> Result<Self, Error>
    where
        R: io::AsyncRead + Unpin;
}

impl<T, Ctx> AsyncEncode<Ctx> for &T
where
    T: AsyncEncode<Ctx> + Sync,
    Ctx: Send
{
    fn encode<W>(&self, ctx: Ctx, writer: &mut W) -> impl std::future::Future<Output = Result<(), Error>> + Send
    where
        W: io::AsyncWrite + Unpin + Send,
    {
        async move {
            (*self).encode(ctx, writer).await
    }
        }
}

impl<T, Ctx> AsyncEncode<(Len, Ctx)> for [T]
where
    T: AsyncEncode<Ctx>,
    Ctx: Clone,
{
    /// Encodes each element of the vector in order.
    ///
    /// If length is also to be encoded, it has to be done separately.
    ///
    /// The length context is provided as a sanity check to protect against logic errors; if the
    /// provided length context is not equal to the vector's length, then this function will return
    /// an error.
    fn encode<W>(&self, (Len(len), inner_ctx): (Len, Ctx), writer: &mut W) -> impl std::future::Future<Output = Result<(), Error>> + Send
    where
        W: io::AsyncWrite + Unpin + Send,
    {
        async move {
        if self.len() != len {
            Err(Error::new(
                "provided length context does not match the slice length",
            ))
        } else {
            self.encode((inner_ctx,), writer).await
        }}
    }
}

impl<T> AsyncEncode<Len> for [T]
where
    T: AsyncEncode + Sync,
{
    /// Encodes each element of the vector in order.
    ///
    /// If length is also to be encoded, it has to be done separately.
    ///
    /// The length context is provided as a sanity check to protect against logic errors; if the
    /// provided length context is not equal to the vector's length, then this function will return
    /// an error.
    fn encode<W>(&self, len: Len, writer: &mut W) -> impl std::future::Future<Output = Result<(), Error>> + Send
    where
        W: io::AsyncWrite + Unpin + Send,
    {
        async move {
            self.encode((len, ()), writer).await
        }
    }
}

impl<T, Ctx> AsyncEncode<(Ctx,)> for [T]
where
    T: AsyncEncode<Ctx> + Sync,
    Ctx: Clone + Send,
{
    /// Encodes each element of the slice in order.
    ///
    /// If length is also to be encoded, it has to be done separately.
    fn encode<W>(&self, (inner_ctx,): (Ctx,), writer: &mut W) -> impl std::future::Future<Output = Result<(), Error>> + Send
    where
        W: io::AsyncWrite + Unpin + Send,
    {
        async move {
        for elem in self {
            elem.encode(inner_ctx.clone(), writer).await?;
        }
        Ok(())
    }
    }
}

impl<T, Ctx, const N: usize> AsyncEncode<Ctx> for [T; N]
where
    T: AsyncEncode<Ctx> + Sync,
    Ctx: Clone + Send,
{
    async fn encode<W>(&self, inner_ctx: Ctx, writer: &mut W) -> Result<(), Error>
    where
        W: io::AsyncWrite + Unpin + Send,
    {
        for elem in self {
            elem.encode(inner_ctx.clone(), writer).await?;
        }
        Ok(())
    }
}

impl<T, Ctx, const N: usize> AsyncDecode<Ctx> for [T; N]
where
    T: AsyncDecode<Ctx> + Copy + Default,
    Ctx: Clone,
{
    async fn decode<R>(inner_ctx: Ctx, reader: &mut R) -> Result<Self, Error>
    where
        R: io::AsyncRead + Unpin,
    {
        //TODO: Use MaybeUninit when stabilized with arrays
        let mut arr = [Default::default(); N];
        for slot in &mut arr {
            *slot = T::decode(inner_ctx.clone(), reader).await?;
        }
        Ok(arr)
    }
}

impl<T, Ctx> AsyncEncode<(Len, Ctx)> for Vec<T>
where
    T: AsyncEncode<Ctx> + Sync,
    Ctx: Clone + Send,
{
    /// Encodes each element of the vector in order.
    ///
    /// If length is also to be encoded, it has to be done separately.
    ///
    /// The length context is provided as a sanity check to protect against logic errors; if the
    /// provided length context is not equal to the vector's length, then this function will return
    /// an error.
    async fn encode<W>(&self, ctx: (Len, Ctx), writer: &mut W) -> Result<(), Error>
    where
        W: io::AsyncWrite + Unpin + Send,
    {
        self.as_slice().encode(ctx, writer).await
    }
}

impl<T> AsyncEncode<Len> for Vec<T>
where
    T: AsyncEncode,
{
    /// Encodes each element of the vector in order.
    ///
    /// If length is also to be encoded, it has to be done separately.
    ///
    /// The length context is provided as a sanity check to protect against logic errors; if the
    /// provided length context is not equal to the vector's length, then this function will return
    /// an error.
    async fn encode<W>(&self, ctx: Len, writer: &mut W) -> Result<(), Error>
    where
        W: io::AsyncWrite + Unpin + Send,
    {
        self.as_slice().encode(ctx, writer).await
    }
}

impl<T, Ctx> AsyncEncode<(Ctx,)> for Vec<T>
where
    T: AsyncEncode<Ctx>,
    Ctx: Clone,
{
    /// Encodes each element of the vector in order.
    ///
    /// If length is also to be encoded, it has to be done separately.
    fn encode<W>(&self, ctx: (Ctx,), writer: &mut W) -> impl std::future::Future<Output = Result<(), Error>>
    where
        W: io::AsyncWrite + Unpin + Send,
    {
        async move {
        self.as_slice().encode(ctx, writer).await
        }
    }
}

impl<T, Ctx> AsyncDecode<(Len, Ctx)> for Vec<T>
where
    T: AsyncDecode<Ctx>,
    Ctx: Clone,
{
    /// Decodes multiple values of type `T`, collecting them in a `Vec`.
    ///
    /// The length of the vector / number of elements decoded is equal to the value of the
    /// `Len` context.
    async fn decode<R>((Len(len), inner_ctx): (Len, Ctx), reader: &mut R) -> Result<Self, Error>
    where
        R: io::AsyncRead + Unpin,
    {
        let mut acc = Self::with_capacity(len);
        for _ in 0..len {
            acc.push(T::decode(inner_ctx.clone(), reader).await?);
        }
        Ok(acc)
    }
}

impl<T> AsyncDecode<Len> for Vec<T>
where
    T: AsyncDecode,
{
    /// Decodes multiple values of type `T`, collecting them in a `Vec`.
    ///
    /// The length of the vector / number of elements decoded is equal to the value of the
    /// `Len` context.
    async fn decode<R>(len: Len, reader: &mut R) -> Result<Self, Error>
    where
        R: io::AsyncRead + Unpin,
    {
        Self::decode((len, ()), reader).await
    }
}

impl<T, Ctx> AsyncEncode<Ctx> for Option<T>
where
    T: AsyncEncode<Ctx> + Send + Sync,
    Ctx: Send
{
    /// If `Some`, then the inner value is encoded, otherwise, nothing is written.
    fn encode<W>(&self, inner_ctx: Ctx, writer: &mut W) -> impl std::future::Future<Output = Result<(), Error>> + Send
    where
        W: io::AsyncWrite + Unpin + Send,
    {async move {
        if let Some(inner) = self {
            inner.encode(inner_ctx, writer).await
        } else {
            Ok(())
        }
    }}
}

impl<T, Ctx> AsyncDecode<Ctx> for Option<T>
where
    T: AsyncDecode<Ctx>,
{
    /// Decodes a value of type `T` and wraps it in `Some`.
    ///
    /// Detecting and deserializing a `None` should be done outside of this function by
    /// checking the relevant conditions in other decoded values and skipping this call if a
    /// `None` is expected.
    ///
    /// Since serializing a `None` writes nothing, deserialization is also a no-op; just construct
    /// a value of `None`.
    async fn decode<R>(inner_ctx: Ctx, reader: &mut R) -> Result<Self, Error>
    where
        R: io::AsyncRead + Unpin,
    {
        T::decode(inner_ctx, reader).await.map(Some)
    }
}

impl<T, Ctx> AsyncDecode<Ctx> for Cow<'_, T>
where
    T: ToOwned + ?Sized,
    T::Owned: AsyncDecode<Ctx>,
{
    /// Decodes a value of type `T::Owned`.
    async fn decode<R>(inner_ctx: Ctx, reader: &mut R) -> Result<Self, Error>
    where
        R: io::AsyncRead + Unpin,
    {
        T::Owned::decode(inner_ctx, reader).await.map(Self::Owned)
    }
}

impl<T, Ctx> AsyncEncode<Ctx> for Box<T>
where
    T: AsyncEncode<Ctx> + std::marker::Sync,
    Ctx: std::marker::Send
{
    /// Encodes the boxed value.
    async fn encode<W>(&self, inner_ctx: Ctx, writer: &mut W) -> Result<(), Error>
    where
        W: io::AsyncWrite + Unpin + std::marker::Send,
    {
        T::encode(self, inner_ctx, writer).await
    }
}

impl<T, Ctx> AsyncDecode<Ctx> for Box<T>
where
    T: AsyncDecode<Ctx>,
{
    /// Decodes a value of type `T` and boxes it.
    async fn decode<R>(inner_ctx: Ctx, reader: &mut R) -> Result<Self, Error>
    where
        R: io::AsyncRead + Unpin,
    {
        T::decode(inner_ctx, reader).await.map(Self::new)
    }
}

impl AsyncEncode for () {
    /// No-op.
    async fn encode<W>(&self, _: (), _: &mut W) -> Result<(), Error>
    where
        W: io::AsyncWrite + Unpin,
    {
        Ok(())
    }
}

impl AsyncDecode for () {
    /// No-op.
    async fn decode<R>(_: (), _: &mut R) -> Result<Self, Error>
    where
        R: io::AsyncRead + Unpin,
    {
        Ok(())
    }
}

macro_rules! impl_primitive {
    ($($t:ty)*) => {$(
        impl AsyncEncode<Endian> for $t {
            async fn encode<W>(&self, endian: Endian, writer: &mut W) -> Result<(), Error>
            where
                W: io::AsyncWrite + Unpin,
            {
                let bytes = match endian {
                    Endian::Big => self.to_be_bytes(),
                    Endian::Little => self.to_le_bytes(),
                };
                writer.write_all(&bytes).await?;
                Ok(())
            }
        }

        impl AsyncDecode<Endian> for $t {
            async fn decode<R>(endian: Endian, reader: &mut R) -> Result<Self, Error>
            where
                R: io::AsyncRead + Unpin,
            {
                let mut bytes = [0u8; mem::size_of::<$t>()];
                reader.read_exact(&mut bytes).await?;
                match endian {
                    Endian::Big => Ok(Self::from_be_bytes(bytes)),
                    Endian::Little => Ok(Self::from_le_bytes(bytes)),
                }
            }
        }
    )*}
}

impl_primitive! {
    u8 u16 u32 u64 u128 i8 i16 i32 i64 i128 f32 f64
}

// Special case: u8/i8 are single-byte values so they can be encoded/decoded without explicit
// endianness context.

impl AsyncEncode for u8 {
    async fn encode<W>(&self, _ctx: (), writer: &mut W) -> Result<(), Error>
    where
        W: io::AsyncWrite + Unpin + std::marker::Send,
    {
        self.encode(Endian::Big, writer).await
    }
}

impl AsyncDecode for u8 {
    async fn decode<R>(_ctx: (), reader: &mut R) -> Result<Self, Error>
    where
        R: io::AsyncRead + Unpin,
    {
        Self::decode(Endian::Big, reader).await
    }
}

impl AsyncEncode for i8 {
    async fn encode<W>(&self, _ctx: (), writer: &mut W) -> Result<(), Error>
    where
        W: io::AsyncWrite + Unpin + std::marker::Send,
    {
        self.encode(Endian::Big, writer).await
    }
}

impl AsyncDecode for i8 {
    async fn decode<R>(_ctx: (), reader: &mut R) -> Result<Self, Error>
    where
        R: io::AsyncRead + Unpin,
    {
        Self::decode(Endian::Big, reader).await
    }
}
