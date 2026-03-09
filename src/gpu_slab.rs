/// Intrusive slab for use on GPU.
/// 
/// This is a simplified slab that doesn't even track occupied/unoccupied slots.
pub struct GpuSlab<T: GpuSlabItem> {
    items: Vec<T>,
    first_free: Option<usize>,
}

/// Trait implemented by user types to expose slab metadata stored inside the struct.
pub trait GpuSlabItem {
    /// Index of next free item in the free list.
    fn next_free(&self) -> Option<usize>;
    fn set_next_free(&mut self, i: Option<usize>);
}

impl<T: GpuSlabItem> GpuSlab<T> {
    /// Create a new empty `GpuSlab` with at least the specified capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self { items: Vec::with_capacity(capacity), first_free: None }
    }

    /// Insert an item in the slab and return an index into it.
    /// 
    /// The index is stable and guaranteed to be valid until [`GpuSlab::remove()`] is called on it.
    pub fn insert(&mut self, item: T) -> usize {
        if let Some(first) = self.first_free {
            let next = self.items[first].next_free();
            self.first_free = next;
            self.items[first] = item;
            return first
        } else {
            let idx = self.items.len();
            self.items.push(item);
            return idx
        }
    }

    /// Remove an item.
    /// 
    /// Removing an already-removed item will either panic or cause incorrect behavior.
    pub fn remove(&mut self, i: usize) {
        let item = &mut self.items[i];
        let next = self.first_free;
        item.set_next_free(next);
        self.first_free = Some(i);
    }

    /// Get a reference to an item.
    pub fn get(&self, i: usize) -> &T {
        return self.items.get(i).unwrap();
    }

    /// Get a mutable reference to an item.
    pub fn get_mut(&mut self, i: usize) -> &mut T {
        return self.items.get_mut(i).unwrap();
    }

    /// Get a reference to the item storage as a slice, including both occupied and unoccupied items.
    pub fn as_slice(&self) -> &[T] {
        &self.items
    }

    /// Get a reference to the item storage as a slice, including both occupied and unoccupied items.
    pub fn len(&self) -> usize {
        self.items.len()
    }
}


// impl GpuSlabItem for crate::BoxData {
//     fn next_free(&self) -> Option<usize> {
//         if self.slab_metadata == u32::MAX {
//             None
//         } else {
//             Some(self.slab_metadata as usize)
//         }
//     }

//     fn set_next_free(&mut self, i: Option<usize>) {
//         match i {
//             Some(i) => self.slab_metadata = i as u32,
//             None => self.slab_metadata = u32::MAX,
//         }
//     }
// }
