use std::num::NonZero;
use std::sync::Arc;

use arrow::array::{ArrayRef, StringArray};
use itertools::Itertools;
use minigu_catalog::label_set::LabelSet;
use minigu_catalog::memory::graph_type::{
    MemoryEdgeTypeCatalog, MemoryGraphTypeCatalog, MemoryVertexTypeCatalog,
};
use minigu_catalog::property::Property;
use minigu_common::data_chunk;
use minigu_common::data_chunk::DataChunk;
use minigu_common::data_type::{DataField, DataSchema, LogicalType};
use minigu_context::graph::{GraphContainer, GraphStorage};
use minigu_context::procedure::Procedure;
use minigu_storage::common::IsolationLevel;
use minigu_storage::model::edge::Edge;
use minigu_storage::model::properties::PropertyRecord;
use minigu_storage::model::vertex::Vertex;
use minigu_storage::tp::MemoryGraph;
use minigu_storage::tp::checkpoint::CheckpointManagerConfig;
use minigu_storage::wal::graph_wal::WalManagerConfig;

fn build_graph_type() -> Arc<MemoryGraphTypeCatalog> {
    let mut graph_type = MemoryGraphTypeCatalog::new();
    let person_label_id = graph_type.add_label("person".into()).unwrap();
    let knows_label_id = graph_type.add_label("knows".into()).unwrap();
    assert_eq!(person_label_id.get(), 1);
    assert_eq!(knows_label_id.get(), 2);
    let person_properties = vec![
        Property::new("id".into(), LogicalType::Int64, false),
        Property::new("name".into(), LogicalType::String, false),
    ];
    let knows_properties = vec![Property::new("since".into(), LogicalType::Int64, true)];
    let person_label_set: LabelSet = [person_label_id].into_iter().collect();
    let knows_label_set: LabelSet = [knows_label_id].into_iter().collect();
    let person_type = Arc::new(MemoryVertexTypeCatalog::new(
        person_label_set.clone(),
        person_properties,
    ));
    let knows_type = Arc::new(MemoryEdgeTypeCatalog::new(
        knows_label_set.clone(),
        person_type.clone(),
        person_type.clone(),
        knows_properties,
    ));
    graph_type.add_vertex_type(person_label_set, person_type);
    graph_type.add_edge_type(knows_label_set, knows_type);
    Arc::new(graph_type)
}

// [(0, 1), (0, 2), (1, 0), (1, 2), (2, 0), (2, 1)]
fn build_graph() -> Arc<MemoryGraph> {
    let checkpoint_config = CheckpointManagerConfig::default();
    let wal_config = WalManagerConfig::default();
    let graph = MemoryGraph::with_config_fresh(checkpoint_config, wal_config);
    let txn = graph.begin_transaction(IsolationLevel::Serializable);
    let vertices = [
        Vertex::new(
            0,
            NonZero::new(1).unwrap(),
            PropertyRecord::new(vec![0.into(), "alice".into()]),
        ),
        Vertex::new(
            1,
            NonZero::new(1).unwrap(),
            PropertyRecord::new(vec![1.into(), "bob".into()]),
        ),
        Vertex::new(
            2,
            NonZero::new(1).unwrap(),
            PropertyRecord::new(vec![2.into(), "charlie".into()]),
        ),
    ];
    let edges = [
        Edge::new(
            0,
            0,
            1,
            NonZero::new(2).unwrap(),
            PropertyRecord::new(vec![123.into()]),
        ),
        Edge::new(
            1,
            0,
            2,
            NonZero::new(2).unwrap(),
            PropertyRecord::new(vec![456.into()]),
        ),
        Edge::new(
            2,
            1,
            0,
            NonZero::new(2).unwrap(),
            PropertyRecord::new(vec![123.into()]),
        ),
        Edge::new(
            3,
            1,
            2,
            NonZero::new(2).unwrap(),
            PropertyRecord::new(vec![789.into()]),
        ),
        Edge::new(
            4,
            2,
            0,
            NonZero::new(2).unwrap(),
            PropertyRecord::new(vec![456.into()]),
        ),
        Edge::new(
            5,
            2,
            1,
            NonZero::new(2).unwrap(),
            PropertyRecord::new(vec![789.into()]),
        ),
    ];
    for vertex in vertices {
        graph.create_vertex(&txn, vertex);
    }
    for edge in edges {
        graph.create_edge(&txn, edge);
    }
    txn.commit().unwrap();
    graph
}

/// Create a test graph with the given name in the current schema.
pub fn build_procedure() -> Procedure {
    let parameters = vec![LogicalType::String];
    Procedure::new(parameters, None, move |context, args| {
        let graph_name = args[0]
            .try_as_string()
            .expect("arg must be a string")
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("graph name cannot be null"))?;
        let schema = context
            .current_schema
            .ok_or_else(|| anyhow::anyhow!("current schema not set"))?;
        let graph = build_graph();
        let graph_type = build_graph_type();
        let container = GraphContainer::new(graph_type, GraphStorage::Memory(graph));
        if !schema.add_graph(graph_name.clone(), Arc::new(container)) {
            return Err(anyhow::anyhow!("graph {graph_name} already exists").into());
        }
        Ok(vec![])
    })
}
