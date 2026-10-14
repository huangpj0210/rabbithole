// Copyright (c) 2019 Parity Technologies (UK) Ltd.
//
// Licensed under the Apache License, Version 2.0 or MIT license, at your option.
//
// A copy of the Apache License, Version 2.0 is included in the software as
// LICENSE-APACHE and a copy of the MIT license is included as
// LICENSE-MIT. You may also obtain a copy of the Apache License, Version 2.0, at
// https://www.apache.org/licenses/LICENSE-2.0, or the MIT license, at
// https://opensource.org/licenses/MIT, at your option.

//! 接收缓冲字节链(rabbithole vendor:Vec→Bytes)。
//!
//! 帧体自 `frame::io` 以零拷贝 `Bytes` 切片交付;此处仅维护链表与剩余计数,
//! 部分消费用 `Bytes::slice`(引用计数切片,不再拷贝)。

use bytes::Bytes;
use std::collections::VecDeque;

/// A sequence of [`Bytes`] chunks.
#[derive(Debug)]
pub(crate) struct Chunks {
    seq: VecDeque<Bytes>,
    len: usize,
}

impl Chunks {
    /// A new empty chunk list.
    pub(crate) fn new() -> Self {
        Chunks {
            seq: VecDeque::new(),
            len: 0,
        }
    }

    /// The total length of bytes yet-to-be-read in all chunks.
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Whether no bytes remain to be read.
    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Add another chunk of bytes to the end.
    pub(crate) fn push(&mut self, x: Bytes) {
        if !x.is_empty() {
            self.len += x.len();
            self.seq.push_back(x);
        }
    }

    /// Remove and return the first chunk.
    pub(crate) fn pop(&mut self) -> Option<Bytes> {
        let chunk = self.seq.pop_front();
        if let Some(c) = &chunk {
            self.len -= c.len();
        }
        chunk
    }

    /// 消费链首 k 字节(k ≤ 链首长度);链首耗尽即出队。
    pub(crate) fn advance_front(&mut self, k: usize) {
        if let Some(front) = self.seq.front_mut() {
            debug_assert!(k <= front.len());
            self.len -= k;
            let rest = front.slice(k..);
            if rest.is_empty() {
                self.seq.pop_front();
            } else {
                *front = rest;
            }
        }
    }

    /// Get a reference to the first chunk.
    pub(crate) fn front(&self) -> Option<&Bytes> {
        self.seq.front()
    }
}
