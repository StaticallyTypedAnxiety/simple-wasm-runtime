use crossbeam::channel::{Receiver, Sender};
use futures::pin_mut;
use std::sync::Arc;
use std::{
    future::Future,
    task::{Context, Poll, Wake},
};

use crate::bindings::wasi::io::poll::{poll, Pollable};

/// The async engine instance
pub struct WasmRuntimeAsyncEngine {
    waker: Arc<FutureWaker>,
    recv: Receiver<()>,
}

/// the reactor that processes poll submissions.
/// TODO: should be given future support
#[derive(Clone)]
pub struct Reactor;

impl Reactor {
    ///calls poll::poll in wasi:io. Useful for finidng out state of subscribed resources
    pub fn poll(polls: &[&Pollable]) -> Vec<u32> {
        poll(polls)
    }
}

impl WasmRuntimeAsyncEngine {
    /// function to execute futures
    pub fn block_on<K, F: Future<Output = K>, Fun: FnOnce(Reactor) -> F>(async_closure: Fun) -> K {
        let reactor = Reactor;
        let future = async_closure(reactor);
        pin_mut!(future);
        let (sender, recv) = crossbeam::channel::unbounded();
        let runtime_engine = WasmRuntimeAsyncEngine {
            waker: Arc::new(FutureWaker(sender.clone())),
            recv,
        };
        let waker = runtime_engine.waker.into();
        let mut context = Context::from_waker(&waker);
        let _ = sender.send(()); //initial send;
        loop {
            if runtime_engine.recv.recv().is_ok() {
                if let Poll::Ready(res) = future.as_mut().poll(&mut context) {
                    return res;
                }
            }
        }
    }
}

struct FutureWaker(Sender<()>);

impl FutureWaker {
    fn wake_inner(&self) {
        let _ = self.0.send(());
    }
}

impl Wake for FutureWaker {
    fn wake(self: std::sync::Arc<Self>) {
        self.wake_inner();
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::future::Future;

    struct CountFuture {
        min: u8,
        max: u8,
    }

    impl Future for CountFuture {
        type Output = u8;
        fn poll(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            let count_fut_mut = self.get_mut();
            if count_fut_mut.min == count_fut_mut.max {
                return std::task::Poll::Ready(count_fut_mut.min);
            }

            count_fut_mut.min += 1;
            cx.waker().wake_by_ref();
            std::task::Poll::Pending
        }
    }

    #[test]
    fn test_enqueue() {
        let (sender, recv) = crossbeam::channel::unbounded();
        let count_future = CountFuture { max: 3, min: 0 };
        let runtime_engine = WasmRuntimeAsyncEngine {
            waker: FutureWaker(sender).into(),
            recv,
        };
        let waker = runtime_engine.waker.into();
        let mut context = Context::from_waker(&waker);
        futures::pin_mut!(count_future);
        let _ = count_future.as_mut().poll(&mut context);
        let _ = count_future.as_mut().poll(&mut context);
        assert_eq!(runtime_engine.recv.len(), 2);
    }

    #[test]
    fn test_block_on() {
        let count_future = CountFuture { max: 3, min: 0 };

        assert_eq!(
            WasmRuntimeAsyncEngine::block_on(|_reactor| async move { count_future.await }),
            3
        );
    }
}
