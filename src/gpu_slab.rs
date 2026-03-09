/// Trait implemented by user types to expose slab metadata stored inside the struct.
trait GpuSlabItem {
    /// Index of next free item in the free list.
    fn next_free(&self) -> u32;
    fn set_next_free(&mut self, idx: u32);

    /// Whether this slot is currently occupied.
    /// 
    /// It's okay to provide "fake" implementations of these under certain circumstances. 
    fn occupied(&self) -> bool;
    fn set_occupied(&mut self, v: bool);
}

/// Intrusive slab with no per-item metadata stored externally.
struct GpuSlab<T: GpuSlabItem> {
    items: Vec<T>,
    free_head: Option<u32>,
}

impl<T: GpuSlabItem> GpuSlab<T> {
    /// Create a new empty `GpuSlab` with at least the specified capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self { items: Vec::with_capacity(capacity), free_head: None }
    }

    /// Insert an item in the slab and return an index into it.
    /// 
    /// The index is stable and guaranteed to be valid until [`GpuSlab::remove()`] is called on it.
    pub fn insert(&mut self, mut item: T) -> u32 {
        item.set_occupied(true);

        if let Some(head) = self.free_head {
            let idx = head as usize;
            let next = self.items[idx].next_free();
            self.free_head = (next != u32::MAX).then_some(next);

            self.items[idx] = item;
            idx as u32
        } else {
            let idx = self.items.len() as u32;
            self.items.push(item);
            idx
        }
    }

    /// Remove an item.
    pub fn remove(&mut self, idx: u32) {
        let i = idx as usize;
        let item = &mut self.items[i];
        item.set_occupied(false);

        let next = self.free_head.unwrap_or(u32::MAX);
        item.set_next_free(next);
        self.free_head = Some(idx);
    }

    pub fn get(&self, idx: u32) -> Option<&T> {
        let item = self.items.get(idx as usize)?;
        item.occupied().then_some(item)
    }

    pub fn get_mut(&mut self, idx: u32) -> Option<&mut T> {
        let item = self.items.get_mut(idx as usize)?;
        item.occupied().then_some(item)
    }

    pub fn as_slice(&self, idx: u32) -> &[T] {
        &self.items
    }
}