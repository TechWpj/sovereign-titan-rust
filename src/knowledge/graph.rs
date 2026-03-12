//! Knowledge Graph — entity-relation graph with traversal and persistence.
//!
//! Ported from `sovereign_titan/knowledge/graph.py`. Uses `petgraph` for
//! the underlying directed graph structure with entity nodes and typed
//! relation edges.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

/// An entity node in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    pub name: String,
    pub entity_type: String,
    pub domain: String,
    pub properties: HashMap<String, String>,
}

/// A typed relation edge between two entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    pub relation_type: String,
    pub properties: HashMap<String, String>,
}

/// Serializable snapshot of the graph for persistence.
#[derive(Debug, Serialize, Deserialize)]
struct GraphSnapshot {
    entities: Vec<Entity>,
    edges: Vec<(String, String, Relation)>, // (source_id, target_id, relation)
}

/// In-memory knowledge graph backed by `petgraph`.
pub struct KnowledgeGraph {
    graph: DiGraph<Entity, Relation>,
    /// Entity ID → NodeIndex mapping for fast lookup.
    index: HashMap<String, NodeIndex>,
    /// Optional persistence path for JSON snapshots.
    persist_path: Option<PathBuf>,
}

impl KnowledgeGraph {
    /// Create an empty graph, optionally loading from a persistence file.
    pub fn new(persist_path: Option<&str>) -> Result<Self> {
        let mut kg = Self {
            graph: DiGraph::new(),
            index: HashMap::new(),
            persist_path: persist_path.map(PathBuf::from),
        };

        if let Some(path) = kg.persist_path.clone() {
            if path.exists() {
                match kg.load_from_file(&path) {
                    Ok(count) => info!("KnowledgeGraph: loaded {count} entities from disk"),
                    Err(e) => warn!("KnowledgeGraph: failed to load from disk: {e}"),
                }
            }
        }

        Ok(kg)
    }

    /// Load graph state from a JSON file.
    fn load_from_file(&mut self, path: &Path) -> Result<usize> {
        let data = fs::read_to_string(path).context("failed to read graph file")?;
        let snapshot: GraphSnapshot = serde_json::from_str(&data).context("failed to parse graph JSON")?;

        for entity in &snapshot.entities {
            let idx = self.graph.add_node(entity.clone());
            self.index.insert(entity.id.clone(), idx);
        }

        for (source_id, target_id, relation) in &snapshot.edges {
            if let (Some(&src), Some(&tgt)) = (self.index.get(source_id), self.index.get(target_id)) {
                self.graph.add_edge(src, tgt, relation.clone());
            }
        }

        Ok(snapshot.entities.len())
    }

    /// Persist the graph to disk as JSON.
    fn persist(&self) -> Result<()> {
        let Some(path) = &self.persist_path else {
            return Ok(());
        };

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let entities: Vec<Entity> = self.graph.node_weights().cloned().collect();

        let edges: Vec<(String, String, Relation)> = self
            .graph
            .edge_indices()
            .filter_map(|e| {
                let (src, tgt) = self.graph.edge_endpoints(e)?;
                let relation = self.graph.edge_weight(e)?.clone();
                let src_id = self.graph.node_weight(src)?.id.clone();
                let tgt_id = self.graph.node_weight(tgt)?.id.clone();
                Some((src_id, tgt_id, relation))
            })
            .collect();

        let snapshot = GraphSnapshot { entities, edges };
        let json = serde_json::to_string_pretty(&snapshot)?;

        let tmp = path.with_extension("tmp");
        fs::write(&tmp, &json)?;
        fs::rename(&tmp, path)?;

        Ok(())
    }

    // ── Public API ───────────────────────────────────────────────────────────

    /// Add an entity to the graph. Returns its ID.
    /// If an entity with the same name and type exists, returns the existing ID.
    pub fn add_entity(
        &mut self,
        name: &str,
        entity_type: &str,
        properties: HashMap<String, String>,
        domain: &str,
    ) -> Result<String> {
        // Dedup: check for existing entity with same name + type.
        if let Some(existing) = self.find_entity(name) {
            if existing.entity_type == entity_type {
                return Ok(existing.id.clone());
            }
        }

        let id = Uuid::new_v4().to_string();
        let entity = Entity {
            id: id.clone(),
            name: name.to_string(),
            entity_type: entity_type.to_string(),
            domain: domain.to_string(),
            properties,
        };

        let idx = self.graph.add_node(entity);
        self.index.insert(id.clone(), idx);
        self.persist()?;

        Ok(id)
    }

    /// Find an entity by name (case-insensitive).
    pub fn find_entity(&self, name: &str) -> Option<&Entity> {
        let lower = name.to_lowercase();
        self.graph
            .node_weights()
            .find(|e| e.name.to_lowercase() == lower)
    }

    /// Get an entity by ID.
    pub fn get_entity(&self, id: &str) -> Option<&Entity> {
        self.index
            .get(id)
            .and_then(|&idx| self.graph.node_weight(idx))
    }

    /// Get all entities of a given type.
    pub fn get_entities_by_type(&self, entity_type: &str) -> Vec<&Entity> {
        self.graph
            .node_weights()
            .filter(|e| e.entity_type == entity_type)
            .collect()
    }

    /// Remove an entity and all its connected edges.
    pub fn remove_entity(&mut self, id: &str) -> Result<bool> {
        if let Some(&idx) = self.index.get(id) {
            self.graph.remove_node(idx);
            self.index.remove(id);
            // Rebuild index since petgraph may reuse indices after removal.
            self.rebuild_index();
            self.persist()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Rebuild the ID → NodeIndex map after structural changes.
    fn rebuild_index(&mut self) {
        self.index.clear();
        for idx in self.graph.node_indices() {
            if let Some(entity) = self.graph.node_weight(idx) {
                self.index.insert(entity.id.clone(), idx);
            }
        }
    }

    /// Add a directed relation between two entities (by ID).
    pub fn add_relation(
        &mut self,
        source_id: &str,
        relation_type: &str,
        target_id: &str,
        properties: HashMap<String, String>,
    ) -> Result<()> {
        let &src = self
            .index
            .get(source_id)
            .ok_or_else(|| anyhow::anyhow!("source entity '{source_id}' not found"))?;
        let &tgt = self
            .index
            .get(target_id)
            .ok_or_else(|| anyhow::anyhow!("target entity '{target_id}' not found"))?;

        let relation = Relation {
            relation_type: relation_type.to_string(),
            properties,
        };

        self.graph.add_edge(src, tgt, relation);
        self.persist()?;
        Ok(())
    }

    /// Get all relations for an entity, optionally filtered by type and direction.
    pub fn get_relations(
        &self,
        entity_id: &str,
        relation_type: Option<&str>,
        direction: Direction,
    ) -> Vec<(&Entity, &Relation)> {
        let Some(&idx) = self.index.get(entity_id) else {
            return Vec::new();
        };

        self.graph
            .edges_directed(idx, direction)
            .filter(|edge| {
                relation_type.map_or(true, |rt| edge.weight().relation_type == rt)
            })
            .filter_map(|edge| {
                let other = match direction {
                    Direction::Outgoing => edge.target(),
                    Direction::Incoming => edge.source(),
                };
                let entity = self.graph.node_weight(other)?;
                Some((entity, edge.weight()))
            })
            .collect()
    }

    /// Get all adjacent entities (both incoming and outgoing) for context enrichment.
    pub fn get_context(&self, entity_name: &str) -> String {
        let Some(entity) = self.find_entity(entity_name) else {
            return format!("No knowledge about '{entity_name}'.");
        };

        let mut context_parts = vec![format!("{} ({})", entity.name, entity.entity_type)];

        let outgoing = self.get_relations(&entity.id, None, Direction::Outgoing);
        for (target, rel) in &outgoing {
            context_parts.push(format!(
                "  → {} → {}",
                rel.relation_type, target.name
            ));
        }

        let incoming = self.get_relations(&entity.id, None, Direction::Incoming);
        for (source, rel) in &incoming {
            context_parts.push(format!(
                "  ← {} ← {}",
                rel.relation_type, source.name
            ));
        }

        context_parts.join("\n")
    }

    /// Get graph statistics.
    pub fn get_stats(&self) -> (usize, usize) {
        (self.graph.node_count(), self.graph.edge_count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_find_entity() {
        let mut kg = KnowledgeGraph::new(None).unwrap();
        let id = kg
            .add_entity("Rust", "language", HashMap::new(), "world")
            .unwrap();
        let entity = kg.find_entity("rust").unwrap();
        assert_eq!(entity.id, id);
        assert_eq!(entity.name, "Rust");
    }

    #[test]
    fn test_dedup_same_name_type() {
        let mut kg = KnowledgeGraph::new(None).unwrap();
        let id1 = kg.add_entity("Rust", "language", HashMap::new(), "world").unwrap();
        let id2 = kg.add_entity("Rust", "language", HashMap::new(), "world").unwrap();
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_add_relation_and_context() {
        let mut kg = KnowledgeGraph::new(None).unwrap();
        let rust_id = kg.add_entity("Rust", "language", HashMap::new(), "world").unwrap();
        let cargo_id = kg.add_entity("Cargo", "tool", HashMap::new(), "world").unwrap();
        kg.add_relation(&rust_id, "has_tool", &cargo_id, HashMap::new()).unwrap();

        let ctx = kg.get_context("Rust");
        assert!(ctx.contains("has_tool"));
        assert!(ctx.contains("Cargo"));
    }

    #[test]
    fn test_remove_entity() {
        let mut kg = KnowledgeGraph::new(None).unwrap();
        let id = kg.add_entity("Temp", "test", HashMap::new(), "world").unwrap();
        assert!(kg.remove_entity(&id).unwrap());
        assert!(kg.find_entity("Temp").is_none());
    }

    #[test]
    fn test_persistence() {
        let dir = std::env::temp_dir().join(format!("titan_kg_test_{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("graph.json");
        let path_str = path.to_str().unwrap();

        {
            let mut kg = KnowledgeGraph::new(Some(path_str)).unwrap();
            kg.add_entity("Python", "language", HashMap::new(), "world").unwrap();
        }

        {
            let kg = KnowledgeGraph::new(Some(path_str)).unwrap();
            assert!(kg.find_entity("Python").is_some());
        }
    }
}
