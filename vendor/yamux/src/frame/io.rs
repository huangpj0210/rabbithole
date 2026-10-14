// Copyright (c) 2019 Parity Technologies (UK) Ltd.
//
// Licensed under the Apache License, Version 2.0 or MIT license, at your option.
//
// A copy of the Apache License, Version 2.0 is included in the software as
// LICENSE-APACHE and a copy of the MIT license is included as
// LICENSE-MIT. You may also obtain a copy of the Apache License, Version 2.0, at
// https://www.apache.org/licenses/LICENSE-2.0, or the MIT license, at
// https://opensource.org/licenses/MIT, at your option.

//! 零拷贝改造说明(rabbithole vendor):
//!
//! * 写侧:帧头(12B)+帧体通过 [`Writev2::poll_write2`] 合并下发
//!   (载体实现可用 `writev` 单 syscall);帧体来自 `crate::pool` 池化缓冲,
//!   写穿后归还。
//! * 读侧:staging `BytesMut` 批量接收,一次 read 可含多帧;帧体以
//!   `BytesMut::split_to(...).freeze()` 零拷贝切片交付,消除逐帧
//!   `vec![0; body_len]` 的分配+清零。

use super::{
    header::{self, HeaderDecodeError},
    Frame,
};
use crate::connection::Id;
use bytes::{Buf, BufMut, BytesMut};
use futures::{prelude::*, ready};
use std::{
    fmt, io,
    pin::Pin,
    task::{Context, Poll},
};

/// Maximum Yamux frame body length
///
/// Limits the amount of bytes a remote can cause the local node to allocate at once when reading.
///
/// Chosen based on intuition in past iterations.
const MAX_FRAME_BODY_LEN: usize = crate::MIB;

/// staging 一次读入的最小预留(批量接收窗口;一次 read 可解出多帧)。
const READ_RESERVE: usize = 64 * 1024;

/// 小帧 copy-out 阈值(≤ READ_RESERVE)。goal-repair C1:帧体不完整时
/// staging 按 `missing.max(READ_RESERVE)` 预留,交错大帧会驱动 backing
/// 扩容迁移;若小帧仍以零拷贝切片交付,未消费的小切片会把整段
/// (不断增长的)staging backing 钉在驻留里。阈值内帧体改为精确小分配,
/// 大帧保留零拷贝切片(转发主力路径不受影响)。
const SMALL_FRAME_COPY: usize = 4 * 1024;

/// 两段缓冲合并写出。
///
/// 由载体侧实现:支持向量写出的 IO 用 `writev` 一次下发(a 为帧头、b 为帧体),
/// 不支持时退化为顺序写。返回值语义与 `poll_write` 一致:针对 a+b 拼接视图
/// 本次写出的字节数;a 为空时只写 b,b 为空时只写 a。
pub trait Writev2: AsyncWrite {
    fn poll_write2(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        a: &[u8],
        b: &[u8],
    ) -> Poll<io::Result<usize>>;
}

/// 顺序写出适配器:把 [`Writev2`] 实现为「先 a 后 b」的两段顺序写。
/// yamux 内部测试与不支持向量写的载体使用。
pub struct Sequential<T>(pub T);

impl<T: AsyncRead + Unpin> AsyncRead for Sequential<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_read(cx, buf)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for Sequential<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_write(cx, buf)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_flush(cx)
    }
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_close(cx)
    }
}

impl<T: AsyncWrite + Unpin> Writev2 for Sequential<T> {
    fn poll_write2(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        a: &[u8],
        b: &[u8],
    ) -> Poll<io::Result<usize>> {
        let inner = &mut self.get_mut().0;
        if !a.is_empty() {
            Pin::new(inner).poll_write(cx, a)
        } else {
            Pin::new(inner).poll_write(cx, b)
        }
    }
}

/// A [`Stream`] and writer of [`Frame`] values.
pub(crate) struct Io<T> {
    id: Id,
    io: T,
    read: Staging,
    write_state: WriteState,
}

/// 接收 staging:批量读入 + 零拷贝帧体切片。
///
/// 初始化不变量:`[0, zeroed)` 相对当前 buf 起点已初始化,且恒有
/// `zeroed >= buf.len()`。向底层 IO 暴露空闲区前,先把将写区间零填,
/// 使「未初始化内存 → `&mut [u8]`」的转换满足 futures `AsyncRead`
/// 的缓冲契约(调用方传入的切片必须已初始化)。
struct Staging {
    buf: BytesMut,
    zeroed: usize,
}

impl Staging {
    fn new() -> Self {
        Self {
            buf: BytesMut::new(),
            zeroed: 0,
        }
    }

    fn len(&self) -> usize {
        self.buf.len()
    }

    fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    fn reserve(&mut self, additional: usize) {
        let cap_before = self.buf.capacity();
        self.buf.reserve(additional);
        if self.buf.capacity() != cap_before {
            // 扩容/搬移只保留 [0, len):其后重新视为未初始化
            self.zeroed = self.zeroed.min(self.buf.len());
        }
    }

    /// 确保空闲区前 `want` 字节已初始化,返回该可写切片
    /// (实际长度受当前空闲容量截断,≥0)。
    fn zeroed_spare(&mut self, want: usize) -> &mut [u8] {
        let len = self.buf.len();
        let fill_from = self.zeroed;
        let spare = self.buf.spare_capacity_mut();
        let take = want.min(spare.len());
        let ptr = spare.as_mut_ptr();
        if fill_from < len + take {
            for b in &mut spare[fill_from - len..take] {
                *b = std::mem::MaybeUninit::new(0);
            }
            self.zeroed = len + take;
        }
        // SAFETY: [len, len+take) ⊆ [0, zeroed) 已初始化(上面刚保证),
        // 构造 &mut [u8] 不违反初始化契约。
        #[allow(unsafe_code)]
        unsafe {
            std::slice::from_raw_parts_mut(ptr.cast::<u8>(), take)
        }
    }

    /// 消费前缀 k 字节(帧头/帧体边界推进)。
    fn advance(&mut self, k: usize) {
        self.buf.advance(k);
        self.zeroed -= k;
    }

    /// 取出前缀 k 字节为帧体。
    /// 小帧(≤ SMALL_FRAME_COPY)copy-out 为精确独立分配,避免未消费的
    /// 小切片钉住 staging backing(C1);大帧保持零拷贝切片。
    fn take_body(&mut self, k: usize) -> bytes::Bytes {
        if k <= SMALL_FRAME_COPY {
            let mut b = BytesMut::with_capacity(k);
            b.extend_from_slice(&self.buf[..k]);
            self.advance(k);
            b.freeze()
        } else {
            self.split_to(k)
        }
    }

    /// 取出前缀 k 字节为零拷贝帧体。
    fn split_to(&mut self, k: usize) -> bytes::Bytes {
        let body = self.buf.split_to(k).freeze();
        self.zeroed -= k;
        body
    }

    /// 读入 n 字节后推进长度(bytes 官方 IO 扩展模式;n ≤ 已初始化区间)。
    fn appended(&mut self, n: usize) {
        #[allow(unsafe_code)]
        unsafe {
            self.buf.advance_mut(n);
        }
    }
}

impl<T: AsyncRead + AsyncWrite + Unpin> Io<T> {
    pub(crate) fn new(id: Id, io: T) -> Self {
        Io {
            id,
            io,
            read: Staging::new(),
            write_state: WriteState::Init,
        }
    }
}

/// The stages of writing a new `Frame`.
///
/// `Combined` 把帧头与帧体视为一个拼接视图,单次 [`Writev2::poll_write2`]
/// 尽可能一次写出;部分写出按 offset 推进。
enum WriteState {
    Init,
    Combined {
        header: [u8; header::HEADER_SIZE],
        body: bytes::Bytes,
        offset: usize,
    },
    Poisoned,
}

impl fmt::Debug for WriteState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            WriteState::Init => f.write_str("(WriteState::Init)"),
            WriteState::Combined { body, offset, .. } => write!(
                f,
                "(WriteState::Combined (offset {offset}) (body-len {}))",
                body.len()
            ),
            WriteState::Poisoned => f.write_str("(WriteState::Poisoned)"),
        }
    }
}

impl WriteState {
    /// 写出剩余字节;返回 Ok(true) 表示本帧已写穿。
    fn poll_write_frame<T: Writev2 + Unpin>(
        &mut self,
        io: &mut T,
        cx: &mut Context<'_>,
    ) -> Poll<Result<bool, io::Error>> {
        let WriteState::Combined {
            header,
            body,
            offset,
        } = self
        else {
            // Init / Poisoned 由调用方处理,不该到达这里
            return Poll::Ready(Ok(true));
        };
        let total = header.len() + body.len();
        loop {
            let (a, b) = if *offset < header.len() {
                (&header[*offset..], &body[..])
            } else {
                (&header[..0], &body[*offset - header.len()..])
            };
            match Pin::new(&mut *io).poll_write2(cx, a, b) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Ready(Ok(0)) => {
                    return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
                }
                Poll::Ready(Ok(n)) => {
                    *offset += n;
                    if *offset > total {
                        let err = io::Error::other(format!(
                            "Writer returned invalid write count n={n}: {offset} > {total}"
                        ));
                        return Poll::Ready(Err(err));
                    }
                    if *offset == total {
                        return Poll::Ready(Ok(true));
                    }
                    // 部分写出:继续写剩余(就绪状态下立即返回)
                }
            }
        }
    }
}

impl<T> Sink<Frame<()>> for Io<T>
where
    T: AsyncRead + AsyncWrite + Unpin + Writev2,
{
    type Error = io::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = Pin::into_inner(self);
        loop {
            log::trace!("{}: write: {:?}", this.id, this.write_state);
            match this.write_state {
                WriteState::Init => return Poll::Ready(Ok(())),
                WriteState::Combined { .. } => {
                    let mut state = std::mem::replace(&mut this.write_state, WriteState::Poisoned);
                    match state.poll_write_frame(&mut this.io, cx) {
                        Poll::Pending => {
                            this.write_state = state;
                            return Poll::Pending;
                        }
                        Poll::Ready(Err(e)) => {
                            this.write_state = WriteState::Poisoned;
                            return Poll::Ready(Err(e));
                        }
                        Poll::Ready(Ok(_done)) => {
                            // 帧写穿:池化帧体按值取回归还(引用计数不为 1 时自然释放)
                            if let WriteState::Combined { body, .. } = state {
                                if let Ok(mut bm) = body.try_into_mut() {
                                    bm.clear();
                                    crate::pool::put(bm);
                                }
                            }
                            this.write_state = WriteState::Init;
                        }
                    }
                }
                WriteState::Poisoned => {
                    return Poll::Ready(Err(io::Error::other(
                        "Sink is in poisoned state due to previous write error",
                    )))
                }
            }
        }
    }

    fn start_send(self: Pin<&mut Self>, f: Frame<()>) -> Result<(), Self::Error> {
        // B4:写错误已置 Poisoned 后必须拒绝新帧——无条件接受会覆盖
        // 半写状态,使帧边界错乱(前帧残段 + 新帧拼接破坏线协议)。
        if matches!(self.write_state, WriteState::Poisoned) {
            return Err(io::Error::other(
                "Sink is in poisoned state due to previous write error",
            ));
        }
        let header = header::encode(&f.header);
        let buffer = f.body;
        self.get_mut().write_state = WriteState::Combined {
            header,
            body: buffer,
            offset: 0,
        };
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = Pin::into_inner(self);
        ready!(this.poll_ready_unpin(cx))?;
        Pin::new(&mut this.io).poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = Pin::into_inner(self);
        ready!(this.poll_ready_unpin(cx))?;
        Pin::new(&mut this.io).poll_close(cx)
    }
}

impl<T: AsyncRead + AsyncWrite + Unpin> Stream for Io<T> {
    type Item = Result<Frame<()>, FrameDecodeError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        loop {
            // 1) staging 中已有完整帧则直接交付(零拷贝切片)
            if this.read.len() >= header::HEADER_SIZE {
                let hdr = {
                    let bytes: &[u8; header::HEADER_SIZE] = this.read.buf[..header::HEADER_SIZE]
                        .try_into()
                        .expect("length checked above");
                    match header::decode(bytes) {
                        Ok(hd) => hd,
                        Err(e) => return Poll::Ready(Some(Err(e.into()))),
                    }
                };
                log::trace!("{}: read: {}", this.id, hdr);

                if hdr.tag() != header::Tag::Data {
                    this.read.advance(header::HEADER_SIZE);
                    return Poll::Ready(Some(Ok(Frame::new(hdr))));
                }

                let body_len = hdr.len().val() as usize;
                if body_len > MAX_FRAME_BODY_LEN {
                    return Poll::Ready(Some(Err(FrameDecodeError::FrameTooLarge(body_len))));
                }

                if this.read.len() - header::HEADER_SIZE >= body_len {
                    this.read.advance(header::HEADER_SIZE);
                    let body = this.read.take_body(body_len);
                    return Poll::Ready(Some(Ok(Frame { header: hdr, body })));
                }
                // 帧体不完整:按缺口预留
                let missing = body_len - (this.read.len() - header::HEADER_SIZE);
                this.read.reserve(missing.max(READ_RESERVE));
            } else {
                // 帧头不完整(或 staging 空):批量预留
                this.read.reserve(READ_RESERVE);
            }

            // 2) 从底层读入 staging 空闲区(已按需零填,满足初始化契约)
            let spare = this.read.zeroed_spare(usize::MAX);
            if spare.is_empty() {
                // reserve 后仍无空闲(理论不可达):自唤醒让出,避免
                // 无 waker 的 Pending 挂死,也不自旋
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            let n = match ready!(Pin::new(&mut this.io).poll_read(cx, spare)) {
                Ok(0) => 0,
                Ok(n) => {
                    this.read.appended(n);
                    n
                }
                Err(e) => return Poll::Ready(Some(Err(FrameDecodeError::Io(e)))),
            };
            if n == 0 {
                // EOF:staging 干净(帧边界)= 正常关闭;残留半帧 = 协议错
                if this.read.is_empty() {
                    return Poll::Ready(None);
                }
                let e = FrameDecodeError::Io(io::ErrorKind::UnexpectedEof.into());
                return Poll::Ready(Some(Err(e)));
            }
        }
    }
}

/// Possible errors while decoding a message frame.
#[non_exhaustive]
#[derive(Debug)]
pub enum FrameDecodeError {
    /// An I/O error.
    Io(io::Error),
    /// Decoding the frame header failed.
    Header(HeaderDecodeError),
    /// A data frame body length is larger than the configured maximum.
    FrameTooLarge(usize),
}

impl std::fmt::Display for FrameDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            FrameDecodeError::Io(e) => write!(f, "i/o error: {e}"),
            FrameDecodeError::Header(e) => write!(f, "decode error: {e}"),
            FrameDecodeError::FrameTooLarge(n) => write!(f, "frame body is too large ({n})"),
        }
    }
}

impl std::error::Error for FrameDecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FrameDecodeError::Io(e) => Some(e),
            FrameDecodeError::Header(e) => Some(e),
            FrameDecodeError::FrameTooLarge(_) => None,
        }
    }
}

impl From<std::io::Error> for FrameDecodeError {
    fn from(e: std::io::Error) -> Self {
        FrameDecodeError::Io(e)
    }
}

impl From<HeaderDecodeError> for FrameDecodeError {
    fn from(e: HeaderDecodeError) -> Self {
        FrameDecodeError::Header(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::header::StreamId;
    use quickcheck::{Arbitrary, Gen, QuickCheck};
    use rand::RngCore;

    impl Arbitrary for Frame<()> {
        fn arbitrary(g: &mut Gen) -> Self {
            let mut header: header::Header<()> = Arbitrary::arbitrary(g);
            let body = if header.tag() == header::Tag::Data {
                header.set_len(header.len().val() % 4096);
                let mut b = vec![0; header.len().val() as usize];
                rand::thread_rng().fill_bytes(&mut b);
                b.into()
            } else {
                bytes::Bytes::new()
            };
            Frame { header, body }
        }
    }

    #[test]
    fn encode_decode_identity() {
        fn property(f: Frame<()>) -> bool {
            futures::executor::block_on(async move {
                let id = crate::connection::Id::next();
                let cursor = futures::io::Cursor::new(Vec::new());
                let mut io = Io::new(id, Sequential(cursor));
                if io.send(f.clone()).await.is_err() {
                    return false;
                }
                if io.flush().await.is_err() {
                    return false;
                }
                io.io.0.set_position(0);
                if let Ok(Some(x)) = io.try_next().await {
                    x == f
                } else {
                    false
                }
            })
        }

        QuickCheck::new()
            .tests(10_000)
            .quickcheck(property as fn(Frame<()>) -> bool)
    }

    /// 写侧恒定失败的载体。
    struct FailingWriter;

    impl AsyncRead for FailingWriter {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &mut [u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Pending
        }
    }

    impl AsyncWrite for FailingWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Err(io::Error::other("mock-io-write-failure")))
        }
        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    /// B4(红→绿):写错误之后 sink 必须 Poisoned——poll_ready 持续 Err,
    /// start_send 拒绝新帧,不得覆盖半写状态后继续接受帧。
    #[test]
    fn write_error_poisons_sink() {
        let mut io = Io::new(crate::connection::Id::next(), Sequential(FailingWriter));

        let frame: Frame<()> = Frame::data(StreamId::new(1), bytes::Bytes::from_static(b"x"))
            .unwrap()
            .into();
        // 先排入一帧(start_send 在无前一帧时合法)
        Sink::start_send(Pin::new(&mut io), frame).expect("start_send with idle sink");

        let w = futures::task::noop_waker();
        let mut cx = Context::from_waker(&w);
        // 写穿尝试 → 载体写失败 → poll_ready 返回 Err
        match Sink::poll_ready(Pin::new(&mut io), &mut cx) {
            Poll::Ready(Err(e)) => {
                assert!(
                    e.to_string().contains("mock-io-write-failure"),
                    "original io error must propagate, got: {e}"
                );
            }
            other => panic!("poll_ready after write error must be Ready(Err), got {other:?}"),
        }
        // Poisoned 持续:再次 poll_ready 仍 Err
        match Sink::poll_ready(Pin::new(&mut io), &mut cx) {
            Poll::Ready(Err(_)) => {}
            other => panic!("poisoned sink must stay Err, got {other:?}"),
        }
        // start_send 必须拒绝(B4 修复点:修复前无条件接受并覆盖 Poisoned)
        let frame2: Frame<()> = Frame::data(StreamId::new(3), bytes::Bytes::from_static(b"y"))
            .unwrap()
            .into();
        assert!(
            Sink::start_send(Pin::new(&mut io), frame2).is_err(),
            "start_send must reject frames after a write error (B4)"
        );
    }

    /// 读侧按脚本逐块返回(支持部分读)、写侧收集的载体。
    struct ScriptedIo {
        blocks: std::collections::VecDeque<Vec<u8>>,
        cur: Vec<u8>,
        off: usize,
        written: Vec<u8>,
    }

    impl AsyncRead for ScriptedIo {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<io::Result<usize>> {
            let this = self.get_mut();
            if this.off >= this.cur.len() {
                this.cur = this.blocks.pop_front().unwrap_or_default();
                this.off = 0;
            }
            if this.off < this.cur.len() {
                let n = (this.cur.len() - this.off).min(buf.len());
                buf[..n].copy_from_slice(&this.cur[this.off..this.off + n]);
                this.off += n;
                return Poll::Ready(Ok(n));
            }
            Poll::Ready(Ok(0))
        }
    }

    impl AsyncWrite for ScriptedIo {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            self.get_mut().written.extend_from_slice(buf);
            Poll::Ready(Ok(buf.len()))
        }
        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    impl Writev2 for ScriptedIo {
        fn poll_write2(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            a: &[u8],
            b: &[u8],
        ) -> Poll<io::Result<usize>> {
            let this = self.get_mut();
            this.written.extend_from_slice(a);
            this.written.extend_from_slice(b);
            let _ = cx;
            Poll::Ready(Ok(a.len() + b.len()))
        }
    }

    /// C1(红→绿):小帧帧体必须 copy-out 交付——未消费的小帧切片
    /// 不得钉住整个 staging backing(交错大帧扩容下驻留无界放大)。
    /// 判据:小帧 body 独占分配(try_into_mut 成功);
    /// 大帧 body 保持零拷贝切片(try_into_mut 失败,与 staging 共享)。
    #[test]
    fn staging_small_frame_bounded() {
        const M: usize = 32;
        let big = vec![7u8; 256 * 1024];
        let mut blocks = std::collections::VecDeque::new();
        let mut small_payloads = Vec::new();
        for i in 0..M {
            // 小帧:1B 数据
            let mut h = header::Header::data(StreamId::new((i + 1) as u32), 1);
            h.syn();
            small_payloads.push(i as u8 + 0xA0);
            let mut blk = header::encode(&h).to_vec();
            blk.push(i as u8 + 0xA0);
            blocks.push_back(blk);
            // 大帧:触发 staging reserve 扩容,使 backing 迁移
            let mut h = header::Header::data(StreamId::new((i + 1) as u32), big.len() as u32);
            h.syn();
            let mut blk = header::encode(&h).to_vec();
            blk.extend_from_slice(&big);
            blocks.push_back(blk);
        }

        let mut io = Io::new(
            crate::connection::Id::next(),
            ScriptedIo {
                blocks,
                cur: Vec::new(),
                off: 0,
                written: Vec::new(),
            },
        );

        let mut small_bodies = Vec::new();
        let mut saw_big = false;
        while let Some(mut frame) = futures::executor::block_on(io.try_next()).unwrap() {
            if frame.header().tag() == header::Tag::Data && frame.body.len() == 1 {
                small_bodies.push(std::mem::take(&mut frame.body));
            } else if frame.body.len() == big.len() {
                let body = std::mem::take(&mut frame.body);
                // 大帧保持零拷贝:与 staging backing 共享,不可 try_into_mut
                assert!(
                    body.try_into_mut().is_err(),
                    "large frames must stay zero-copy slices of the staging buffer"
                );
                saw_big = true;
            }
        }
        assert_eq!(small_bodies.len(), M, "all small frames must be delivered");
        assert!(saw_big, "large frames must be delivered");
        // C1 判据(容量口径):小帧交付后的可变缓冲容量必须处于
        // 精确小分配量级——不得是 staging backing(64KiB 量级)。
        // 交错大帧驱动 reserve 扩容后,零拷贝小切片要么与 staging
        // 共享 backing(try_into_mut Err),要么独占旧 backing
        // (try_into_mut Ok 但 capacity ~READ_RESERVE 量级)——两者都把
        // 1B 载荷钉在 64KiB+ 驻留上;M 个未消费小帧的总驻留无界放大。
        let mut max_small_cap = 0usize;
        for (i, b) in small_bodies.iter_mut().enumerate() {
            assert_eq!(&b[..], &[small_payloads[i]], "payload must round-trip");
            let bm = match std::mem::take(b).try_into_mut() {
                Ok(bm) => bm,
                Err(_) => panic!(
                    "small frame body still shares the staging backing (C1): copy-out expected"
                ),
            };
            assert!(
                bm.capacity() <= SMALL_FRAME_COPY * 2,
                "small frame body pins a {}-byte backing (C1 unbounded residency)",
                bm.capacity()
            );
            max_small_cap = max_small_cap.max(bm.capacity());
        }
        assert!(
            max_small_cap > 0,
            "small bodies must be independently owned"
        );
    }
}
