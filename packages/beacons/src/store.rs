use rusqlite::{Connection, DropBehavior, Transaction, TransactionBehavior};
use std::collections::HashMap;

use crate::observable::{
  crdt::{ObjectGraph, ObjectSet},
  Events, ObservablePersistentState, Port, SetEvent,
};
use crate::persistent::vector_history::VectorHistory;
use crate::{deserialize, serialize};

#[allow(clippy::type_complexity)]
#[derive(Debug, Clone)]
struct EventBus {
  atoms: Vec<(Port, Option<Vec<u8>>)>,
  nodes: Vec<(Port, Option<u64>)>,
  edges: Vec<(Port, Option<(u128, u64, u128)>)>,
  id_sets: Vec<(Port, SetEvent<u128>)>,
}

impl EventBus {
  fn new() -> Self {
    Self { atoms: Vec::new(), nodes: Vec::new(), edges: Vec::new(), id_sets: Vec::new() }
  }
}

impl Default for EventBus {
  fn default() -> Self {
    Self::new()
  }
}

impl Events<Option<Vec<u8>>> for EventBus {
  fn push(&mut self, port: Port, event: Option<Vec<u8>>) {
    self.atoms.push((port, event));
  }
}

impl Events<Option<u64>> for EventBus {
  fn push(&mut self, port: Port, event: Option<u64>) {
    self.nodes.push((port, event));
  }
}

impl Events<Option<(u128, u64, u128)>> for EventBus {
  fn push(&mut self, port: Port, event: Option<(u128, u64, u128)>) {
    self.edges.push((port, event));
  }
}

impl Events<SetEvent<u128>> for EventBus {
  fn push(&mut self, port: Port, event: SetEvent<u128>) {
    self.id_sets.push((port, event));
  }
}

#[derive(Debug)]
pub struct Store {
  connection: Connection,
  // name: String,
  vector_history: VectorHistory,

  atoms: ObjectSet<EventBus>,
  graph: ObjectGraph<EventBus>,
  event_bus: EventBus,
}

/// Starts an *auto-commit* transaction.
fn txn(connection: &mut Connection) -> Transaction<'_> {
  let mut res = connection.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
  res.set_drop_behavior(DropBehavior::Commit);
  res
}

impl Store {
  pub fn new(mut connection: Connection, name: &'static str) -> Self {
    let mut txn = txn(&mut connection);
    let atoms = ObjectSet::new(&mut txn, name, "atoms");
    let graph = ObjectGraph::new(&mut txn, name, "graph");
    let vector_history = VectorHistory::new(&mut txn, name);
    std::mem::drop(txn);
    Self { connection, vector_history, atoms, graph, event_bus: EventBus::new() }
  }

  pub fn node(&mut self, id: u128) -> Option<u64> {
    self.graph.node(&mut txn(&mut self.connection), id)
  }
  pub fn atom(&mut self, id: u128) -> Option<&[u8]> {
    self.atoms.get(&mut txn(&mut self.connection), id)
  }
  pub fn edge(&mut self, id: u128) -> Option<(u128, u64, u128)> {
    self.graph.edge(&mut txn(&mut self.connection), id)
  }
  pub fn query_edge_src(&mut self, src: u128) -> Vec<u128> {
    self.graph.query_edge_src(&mut txn(&mut self.connection), src)
  }
  pub fn query_edge_src_label(&mut self, src: u128, label: u64) -> Vec<u128> {
    self.graph.query_edge_src_label(&mut txn(&mut self.connection), src, label)
  }
  pub fn query_edge_dst_label(&mut self, dst: u128, label: u64) -> Vec<u128> {
    self.graph.query_edge_dst_label(&mut txn(&mut self.connection), dst, label)
  }

  pub fn set_node(&mut self, id: u128, value: Option<u64>) {
    let mut txn = txn(&mut self.connection);
    let action = self.graph.action_node(&mut txn, id, value);
    let this = self.vector_history.this();
    let next = self.vector_history.next_this() + 1;
    if self.vector_history.push(&mut txn, (this, next, String::from("graph"), serialize(&action).unwrap())).is_some() {
      self.graph.apply(&mut txn, &mut self.event_bus, action);
    }
  }
  pub fn set_atom(&mut self, id: u128, value: Option<Vec<u8>>) {
    let mut txn = txn(&mut self.connection);
    let action = self.atoms.action(&mut txn, id, value);
    let this = self.vector_history.this();
    let next = self.vector_history.next_this() + 1;
    if self.vector_history.push(&mut txn, (this, next, String::from("atoms"), serialize(&action).unwrap())).is_some() {
      self.atoms.apply(&mut txn, &mut self.event_bus, action);
    }
  }
  pub fn set_edge(&mut self, id: u128, value: Option<(u128, u64, u128)>) {
    let mut txn = txn(&mut self.connection);
    let action = self.graph.action_edge(&mut txn, id, value);
    let this = self.vector_history.this();
    let next = self.vector_history.next_this() + 1;
    if self.vector_history.push(&mut txn, (this, next, String::from("graph"), serialize(&action).unwrap())).is_some() {
      self.graph.apply(&mut txn, &mut self.event_bus, action);
    }
  }
  pub fn set_edge_dst(&mut self, id: u128, dst: u128) {
    if let Some((src, label, _)) = self.edge(id) {
      self.set_edge(id, Some((src, label, dst)));
    }
  }

  pub fn subscribe_node(&mut self, id: u128, port: u64) {
    self.graph.subscribe_node(&mut txn(&mut self.connection), &mut self.event_bus, id, port);
  }
  pub fn unsubscribe_node(&mut self, id: u128, port: u64) {
    self.graph.unsubscribe_node(id, port);
  }
  pub fn subscribe_atom(&mut self, id: u128, port: u64) {
    self.atoms.subscribe(&mut txn(&mut self.connection), &mut self.event_bus, id, port);
  }
  pub fn unsubscribe_atom(&mut self, id: u128, port: u64) {
    self.atoms.unsubscribe(id, port);
  }
  pub fn subscribe_edge(&mut self, id: u128, port: u64) {
    self.graph.subscribe_edge(&mut txn(&mut self.connection), &mut self.event_bus, id, port);
  }
  pub fn unsubscribe_edge(&mut self, id: u128, port: u64) {
    self.graph.unsubscribe_edge(id, port);
  }
  pub fn subscribe_multiedge(&mut self, src: u128, label: u64, port: u64) {
    self.graph.subscribe_multiedge(&mut txn(&mut self.connection), &mut self.event_bus, src, label, port);
  }
  pub fn unsubscribe_multiedge(&mut self, src: u128, label: u64, port: u64) {
    self.graph.unsubscribe_multiedge(src, label, port);
  }
  pub fn subscribe_backedge(&mut self, dst: u128, label: u64, port: u64) {
    self.graph.subscribe_backedge(&mut txn(&mut self.connection), &mut self.event_bus, dst, label, port);
  }
  pub fn unsubscribe_backedge(&mut self, dst: u128, label: u64, port: u64) {
    self.graph.unsubscribe_backedge(dst, label, port);
  }

  pub fn sync_serial(&mut self) -> Vec<u8> {
    let nexts = self.vector_history.nexts();
    serialize::<HashMap<u64, u64>>(&nexts).unwrap()
  }
  pub fn sync_actions(&mut self, nexts: &[u8]) -> Vec<u8> {
    let clocks = deserialize::<HashMap<u64, u64>>(nexts).unwrap();
    let actions = self.vector_history.collect(&mut txn(&mut self.connection), clocks);
    serialize::<Vec<(u64, u64, String, Vec<u8>)>>(&actions).unwrap()
  }
  pub fn sync_apply(&mut self, actions: &[u8]) {
    let mut txn = txn(&mut self.connection);
    let actions = deserialize::<Vec<(u64, u64, String, Vec<u8>)>>(actions).unwrap();
    for (_replica, _clock, name, action) in self.vector_history.append(&mut txn, actions) {
      match name.as_str() {
        "atoms" => {
          let action = deserialize(&action).unwrap();
          self.atoms.apply(&mut txn, &mut self.event_bus, action);
        }
        "graph" => {
          let action = deserialize(&action).unwrap();
          self.graph.apply(&mut txn, &mut self.event_bus, action);
        }
        _ => {}
      }
    }
  }
}
