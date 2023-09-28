use rusqlite::{OptionalExtension, Result, Row};
use std::collections::{btree_map::Entry, BTreeMap};

use super::metadata::{StructureMetadata, StructureMetadataTransactor};
use crate::Transactor;

/// A last-writer-wins element set for storing atomic data.
#[derive(Debug)]
pub struct AtomSet {
  metadata: StructureMetadata,
  mods: BTreeMap<u128, (Option<Item>, Item)>,
}

/// `(bucket, clock, (src, label, value))`.
type Item = (u64, u64, Option<(u128, u64, Box<[u8]>)>);

/// Database interface for [`AtomSet`].
pub trait AtomSetTransactor: StructureMetadataTransactor {
  fn init(&mut self, prefix: &str, name: &str);
  fn get(&self, prefix: &str, name: &str, id: u128) -> Option<Item>;
  fn set(&mut self, prefix: &str, name: &str, id: u128, item: &Item);
  fn id_label_value_by_src(&self, prefix: &str, name: &str, src: u128) -> BTreeMap<u128, (u64, Box<[u8]>)>;
  fn id_value_by_src_label(&self, prefix: &str, name: &str, src: u128, label: u64) -> BTreeMap<u128, Box<[u8]>>;
  fn id_src_value_by_label(&self, prefix: &str, name: &str, label: u64) -> BTreeMap<u128, (u128, Box<[u8]>)>;
  fn id_src_by_label_value(&self, prefix: &str, name: &str, label: u64, value: &[u8]) -> BTreeMap<u128, u128>;
  fn by_bucket_clock_range(&self, prefix: &str, name: &str, bucket: u64, lower: Option<u64>) -> BTreeMap<u128, Item>;
}

impl AtomSet {
  /// Creates or loads data.
  pub fn new(prefix: &'static str, name: &'static str, txr: &mut impl AtomSetTransactor) -> Self {
    let metadata = StructureMetadata::new(prefix, name, txr);
    let mods = BTreeMap::new();
    txr.init(prefix, name);
    Self { metadata, mods }
  }

  /// Returns the name of the workspace.
  pub fn prefix(&self) -> &'static str {
    self.metadata.prefix()
  }

  /// Returns the name of the structure.
  pub fn name(&self) -> &'static str {
    self.metadata.name()
  }

  /// Returns the current clock values for each bucket.
  pub fn buckets(&self) -> BTreeMap<u64, u64> {
    self.metadata.buckets()
  }

  /// Returns the largest clock value across all buckets plus one.
  pub fn next(&self) -> u64 {
    self.metadata.next()
  }

  /// Returns pending modifications.
  pub fn mods(&self) -> Vec<(u128, Option<(u128, u64, Box<[u8]>)>, Option<(u128, u64, Box<[u8]>)>)> {
    let mut res = Vec::new();
    for (id, (prev, curr)) in &self.mods {
      res.push((*id, prev.clone().and_then(|(_, _, slv)| slv), curr.clone().2));
    }
    res
  }

  pub fn get(&self, txr: &impl AtomSetTransactor, id: u128) -> Option<Item> {
    self.mods.get(&id).map_or_else(|| txr.get(self.prefix(), self.name(), id), |(_, curr)| Some(curr.clone()))
  }

  pub fn id_label_value_by_src(&self, txr: &impl AtomSetTransactor, src: u128) -> BTreeMap<u128, (u64, Box<[u8]>)> {
    let mut res = txr.id_label_value_by_src(self.prefix(), self.name(), src);
    for (id, (_, (_, _, slv))) in &self.mods {
      match slv {
        Some((src_, label, value)) if src_ == &src => res.insert(*id, (*label, value.clone())),
        _ => res.remove(id),
      };
    }
    res
  }

  pub fn id_value_by_src_label(
    &self,
    txr: &impl AtomSetTransactor,
    src: u128,
    label: u64,
  ) -> BTreeMap<u128, Box<[u8]>> {
    let mut res = txr.id_value_by_src_label(self.prefix(), self.name(), src, label);
    for (id, (_, (_, _, slv))) in &self.mods {
      match slv {
        Some((src_, label_, value)) if src_ == &src && label_ == &label => res.insert(*id, value.clone()),
        _ => res.remove(id),
      };
    }
    res
  }

  pub fn id_src_value_by_label(&self, txr: &impl AtomSetTransactor, label: u64) -> BTreeMap<u128, (u128, Box<[u8]>)> {
    let mut res = txr.id_src_value_by_label(self.prefix(), self.name(), label);
    for (id, (_, (_, _, slv))) in &self.mods {
      match slv {
        Some((src, label_, value)) if label_ == &label => res.insert(*id, (*src, value.clone())),
        _ => res.remove(id),
      };
    }
    res
  }

  pub fn id_src_by_label_value(&self, txr: &impl AtomSetTransactor, label: u64, value: &[u8]) -> BTreeMap<u128, u128> {
    let mut res = txr.id_src_by_label_value(self.prefix(), self.name(), label, value);
    for (id, (_, (_, _, slv))) in &self.mods {
      match slv {
        Some((src, label_, value_)) if label_ == &label && value_.as_ref() == value => res.insert(*id, *src),
        _ => res.remove(id),
      };
    }
    res
  }

  /// Returns all actions strictly later than given clock values.
  /// Absent entries are assumed to be `None`.
  pub fn actions(&self, txr: &impl AtomSetTransactor, version: BTreeMap<u64, u64>) -> BTreeMap<u128, Item> {
    let mut res = BTreeMap::new();
    for &bucket in self.buckets().keys() {
      let lower = version.get(&bucket).copied();
      for (id, item) in txr.by_bucket_clock_range(self.prefix(), self.name(), bucket, lower) {
        res.insert(id, item);
      }
    }
    for (id, (_, item)) in &self.mods {
      let (bucket, clock, _) = item;
      if Some(clock) > version.get(bucket) {
        res.insert(*id, item.clone());
      } else {
        res.remove(id);
      }
    }
    res
  }

  /// Modifies item.
  pub fn set(
    &mut self,
    txr: &impl AtomSetTransactor,
    id: u128,
    bucket: u64,
    clock: u64,
    slv: Option<(u128, u64, Box<[u8]>)>,
  ) -> bool {
    if self.metadata.update(bucket, clock) {
      match self.mods.entry(id) {
        Entry::Vacant(entry) => {
          let prev = txr.get(self.metadata.prefix(), self.metadata.name(), id);
          entry.insert((prev, (bucket, clock, slv)));
        }
        Entry::Occupied(mut entry) => {
          entry.get_mut().1 = (bucket, clock, slv);
        }
      }
      return true;
    }
    false
  }

  /// Saves and returns all pending modifications.
  pub fn save(&mut self, txr: &mut impl AtomSetTransactor) -> BTreeMap<u128, (Option<Item>, Item)> {
    self.metadata.save(txr);
    for (id, (_, curr)) in &self.mods {
      txr.set(self.prefix(), self.name(), *id, curr);
    }
    std::mem::take(&mut self.mods)
  }
}

fn read_row(row: &Row<'_>) -> (u128, Item) {
  let id = row.get(0).unwrap();
  let bucket = row.get(1).unwrap();
  let clock = row.get(2).unwrap();
  let src: Option<_> = row.get(3).unwrap();
  let label: Option<_> = row.get(4).unwrap();
  let value: Option<Vec<u8>> = row.get(5).unwrap();
  (
    u128::from_be_bytes(id),
    (
      u64::from_be_bytes(bucket),
      u64::from_be_bytes(clock),
      value.map(|vec| (u128::from_be_bytes(src.unwrap()), u64::from_be_bytes(label.unwrap()), vec.into())),
    ),
  )
}

fn read_row_id_label_value(row: &Row<'_>) -> (u128, (u64, Box<[u8]>)) {
  let id = row.get(0).unwrap();
  let label = row.get(1).unwrap();
  let value: Vec<u8> = row.get(2).unwrap();
  (u128::from_be_bytes(id), (u64::from_be_bytes(label), value.into()))
}

fn read_row_id_value(row: &Row<'_>) -> (u128, Box<[u8]>) {
  let id = row.get(0).unwrap();
  let value: Vec<u8> = row.get(1).unwrap();
  (u128::from_be_bytes(id), value.into())
}

fn read_row_id_src_value(row: &Row<'_>) -> (u128, (u128, Box<[u8]>)) {
  let id = row.get(0).unwrap();
  let src = row.get(1).unwrap();
  let value: Vec<u8> = row.get(2).unwrap();
  (u128::from_be_bytes(id), (u128::from_be_bytes(src), value.into()))
}

fn read_row_id_src(row: &Row<'_>) -> (u128, u128) {
  let id = row.get(0).unwrap();
  let src = row.get(1).unwrap();
  (u128::from_be_bytes(id), u128::from_be_bytes(src))
}

#[allow(clippy::type_complexity)]
fn make_row(id: u128, item: &Item) -> ([u8; 16], [u8; 8], [u8; 8], Option<[u8; 16]>, Option<[u8; 8]>, Option<&[u8]>) {
  let (bucket, clock, slv) = item;
  let slv = slv.as_ref();
  (
    id.to_be_bytes(),
    bucket.to_be_bytes(),
    clock.to_be_bytes(),
    slv.map(|(src, _, _)| src.to_be_bytes()),
    slv.map(|(_, label, _)| label.to_be_bytes()),
    slv.map(|(_, _, value)| value.as_ref()),
  )
}

impl AtomSetTransactor for Transactor {
  fn init(&mut self, prefix: &str, name: &str) {
    self
      .execute_batch(&format!(
        "
        CREATE TABLE IF NOT EXISTS \"{prefix}.{name}.data\" (
          id BLOB NOT NULL,
          bucket BLOB NOT NULL,
          clock BLOB NOT NULL,
          src BLOB,
          label BLOB,
          value BLOB,
          PRIMARY KEY (id)
        ) STRICT, WITHOUT ROWID;

        CREATE INDEX IF NOT EXISTS \"{prefix}.{name}.data.idx_src_label\" ON \"{prefix}.{name}.data\" (src, label);
        CREATE INDEX IF NOT EXISTS \"{prefix}.{name}.data.idx_label_value\" ON \"{prefix}.{name}.data\" (label, value);
        CREATE INDEX IF NOT EXISTS \"{prefix}.{name}.data.idx_bucket_clock\" ON \"{prefix}.{name}.data\" (bucket, clock);
        "
      ))
      .unwrap();
  }

  fn get(&self, prefix: &str, name: &str, id: u128) -> Option<Item> {
    self
      .prepare_cached(&format!(
        "SELECT id, bucket, clock, src, label, value FROM \"{prefix}.{name}.data\"
        WHERE id = ?"
      ))
      .unwrap()
      .query_row((id.to_be_bytes(),), |row| Ok(read_row(row)))
      .optional()
      .unwrap()
      .map(|(_, item)| item)
  }

  fn set(&mut self, prefix: &str, name: &str, id: u128, item: &Item) {
    self
      .prepare_cached(&format!("REPLACE INTO \"{prefix}.{name}.data\" VALUES (?, ?, ?, ?, ?, ?)"))
      .unwrap()
      .execute(make_row(id, item))
      .unwrap();
  }

  fn id_label_value_by_src(&self, prefix: &str, name: &str, src: u128) -> BTreeMap<u128, (u64, Box<[u8]>)> {
    self
      .prepare_cached(&format!(
        "SELECT id, label, value FROM \"{prefix}.{name}.data\" INDEXED BY \"{prefix}.{name}.data.idx_src_label\"
        WHERE src = ?"
      ))
      .unwrap()
      .query_map((src.to_be_bytes(),), |row| Ok(read_row_id_label_value(row)))
      .unwrap()
      .map(Result::unwrap)
      .collect()
  }

  fn id_value_by_src_label(&self, prefix: &str, name: &str, src: u128, label: u64) -> BTreeMap<u128, Box<[u8]>> {
    self
      .prepare_cached(&format!(
        "SELECT id, value FROM \"{prefix}.{name}.data\" INDEXED BY \"{prefix}.{name}.data.idx_src_label\"
        WHERE src = ? AND label = ?"
      ))
      .unwrap()
      .query_map((src.to_be_bytes(), label.to_be_bytes()), |row| Ok(read_row_id_value(row)))
      .unwrap()
      .map(Result::unwrap)
      .collect()
  }

  fn id_src_value_by_label(&self, prefix: &str, name: &str, label: u64) -> BTreeMap<u128, (u128, Box<[u8]>)> {
    self
      .prepare_cached(&format!(
        "SELECT id, src, value FROM \"{prefix}.{name}.data\" INDEXED BY \"{prefix}.{name}.data.idx_label_value\"
        WHERE label = ?"
      ))
      .unwrap()
      .query_map((label.to_be_bytes(),), |row| Ok(read_row_id_src_value(row)))
      .unwrap()
      .map(Result::unwrap)
      .collect()
  }

  fn id_src_by_label_value(&self, prefix: &str, name: &str, label: u64, value: &[u8]) -> BTreeMap<u128, u128> {
    self
      .prepare_cached(&format!(
        "SELECT id, src FROM \"{prefix}.{name}.data\" INDEXED BY \"{prefix}.{name}.data.idx_label_value\"
        WHERE label = ? AND value = ?"
      ))
      .unwrap()
      .query_map((label.to_be_bytes(), value), |row| Ok(read_row_id_src(row)))
      .unwrap()
      .map(Result::unwrap)
      .collect()
  }

  fn by_bucket_clock_range(&self, prefix: &str, name: &str, bucket: u64, lower: Option<u64>) -> BTreeMap<u128, Item> {
    self
      .prepare_cached(&format!(
        "SELECT id, bucket, clock, src, label, value FROM \"{prefix}.{name}.data\" INDEXED BY \"{prefix}.{name}.data.idx_bucket_clock\"
        WHERE bucket = ? AND clock > ?"
      ))
      .unwrap()
      .query_map((bucket.to_be_bytes(), lower.map(u64::to_be_bytes)), |row| Ok(read_row(row)))
      .unwrap()
      .map(Result::unwrap)
      .collect()
  }
}