use crate::db::{Batch, Database, Iterator};
use nitro_primitives::dbkeys::uint64_to_key;

pub fn delete_starting_at<D: Database>(
    db: &D,
    batch: &mut dyn Batch,
    prefix: &[u8],
    start: &[u8],
) -> anyhow::Result<()> {
    let mut it = db.new_iterator(prefix, start);
    while it.next() {
        let key = it.key();
        batch.delete(key)?;
    }
    if let Some(err) = it.error() {
        return Err(err);
    }
    it.release();
    Ok(())
}
