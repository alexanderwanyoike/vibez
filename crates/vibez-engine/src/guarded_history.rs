//! Bounded musical-coordinate history rejects unavailable or overwritten slots.

#[derive(Debug)]
pub(crate) struct GuardedHistory<T> {
    values: Vec<T>,
    written: u64,
}

impl<T: Copy + Default> GuardedHistory<T> {
    pub(crate) fn required_count(delay: u32, extra_frames: usize) -> Result<usize, &'static str> {
        (delay as usize)
            .checked_add(extra_frames)
            .and_then(|count| count.checked_add(1))
            .ok_or("Position history size overflow")
    }
    pub(crate) fn prepare(count: usize) -> Result<Self, &'static str> {
        if count == 0 {
            return Err("Position history must retain at least one sample");
        }
        count
            .checked_mul(std::mem::size_of::<T>())
            .ok_or("Position history size overflow")?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|_| "Unable to allocate position history")?;
        values.resize(count, T::default());
        Ok(Self { values, written: 0 })
    }
    pub(crate) fn storage_bytes(&self) -> usize {
        self.values.len() * std::mem::size_of::<T>()
    }
    pub(crate) fn written(&self) -> u64 {
        self.written
    }
    pub(crate) fn clear(&mut self) {
        self.written = 0;
    }
    pub(crate) fn retain_from(&mut self, previous: &mut Self) -> bool {
        if self.values.len() != previous.values.len() {
            return false;
        }
        std::mem::swap(&mut self.values, &mut previous.values);
        self.written = previous.written;
        true
    }
    pub(crate) fn push(&mut self, position: T) {
        let index = (self.written % self.values.len() as u64) as usize;
        self.values[index] = position;
        self.written += 1;
    }
    pub(crate) fn get(&self, index: u64) -> Option<T> {
        if index >= self.written || self.written - index > self.values.len() as u64 {
            return None;
        }
        Some(self.values[(index % self.values.len() as u64) as usize])
    }
    pub(crate) fn before_block(&self, delay: u32) -> Option<T> {
        if delay == 0 {
            return None;
        }
        self.written
            .checked_sub(delay as u64)
            .and_then(|index| self.get(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overwritten_unrecorded_and_out_of_capacity_history_is_unavailable() {
        let mut ring = GuardedHistory::<u64>::prepare(3).unwrap();
        assert_eq!(ring.before_block(1), None);
        for position in 10..15 {
            ring.push(position);
        }
        assert_eq!(ring.before_block(0), None);
        assert_eq!(ring.before_block(1), Some(14));
        assert_eq!(ring.before_block(3), Some(12));
        assert_eq!(ring.before_block(4), None);
        assert_eq!(ring.get(1), None);
        assert_eq!(ring.get(5), None);
        ring.clear();
        assert_eq!(ring.before_block(1), None);
        ring.push(90);
        assert_eq!(ring.before_block(1), Some(90));
    }
}
