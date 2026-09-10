use ringbuf::{storage::Heap, traits::Split, SharedRb};
use std::sync::Arc;

pub type Prod = ringbuf::wrap::caching::Caching<Arc<SharedRb<Heap<f32>>>, true, false>;
pub type Cons = ringbuf::wrap::caching::Caching<Arc<SharedRb<Heap<f32>>>, false, true>;

pub struct AudioRingBuffer {
    pub producer: Prod,
    pub consumer: Cons,
}

impl AudioRingBuffer {
    pub fn new(capacity: usize) -> Self {
        let rb = ringbuf::HeapRb::<f32>::new(capacity);
        let (producer, consumer) = rb.split();
        Self { producer, consumer }
    }
}
