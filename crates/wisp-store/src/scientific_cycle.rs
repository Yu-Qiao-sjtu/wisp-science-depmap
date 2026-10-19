use crate::Store;
use anyhow::{bail, Context, Result};
use sqlx::Row;
use wisp_dto::{
    status_transition_allowed, validate_scientific_edges, validate_scientific_node, ScientificEdge,
    ScientificEdgeKind, ScientificNode, ScientificNodeKind, ScientificNodeStatus,
};

impl Store {
    pub async fn insert_scientific_node(&self, node: &ScientificNode) -> Result<()> {
        validate_scientific_node(node).map_err(|error| anyhow::anyhow!(error))?;
        let refs_json = serde_json::to_string(&node.refs)?;
        sqlx::query(
            "INSERT INTO scientific_cycle_nodes(\
               id,cycle_id,project_id,frame_id,kind,status,payload_json,refs_json,created_at,updated_at\
             ) VALUES(?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&node.id)
        .bind(&node.cycle_id)
        .bind(&node.project_id)
        .bind(&node.frame_id)
        .bind(node.kind.as_str())
        .bind(node.status.as_str())
        .bind(&node.payload_json)
        .bind(refs_json)
        .bind(node.created_at)
        .bind(node.updated_at)
        .execute(&self.pool)
        .await
        .context("insert scientific cycle node")?;
        Ok(())
    }

    pub async fn get_scientific_node(&self, id: &str) -> Result<Option<ScientificNode>> {
        let row = sqlx::query(
            "SELECT id,cycle_id,project_id,frame_id,kind,status,payload_json,refs_json,created_at,updated_at \
             FROM scientific_cycle_nodes WHERE id=?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(node_from_row).transpose()
    }

    pub async fn list_scientific_cycle(&self, cycle_id: &str) -> Result<Vec<ScientificNode>> {
        let rows = sqlx::query(
            "SELECT id,cycle_id,project_id,frame_id,kind,status,payload_json,refs_json,created_at,updated_at \
             FROM scientific_cycle_nodes WHERE cycle_id=? ORDER BY created_at,id",
        )
        .bind(cycle_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(node_from_row).collect()
    }

    pub async fn transition_scientific_status(
        &self,
        id: &str,
        status: ScientificNodeStatus,
        updated_at: i64,
    ) -> Result<ScientificNode> {
        let Some(current) = self.get_scientific_node(id).await? else {
            bail!("scientific node {id} was not found");
        };
        if !status_transition_allowed(current.status, status) {
            bail!(
                "scientific status cannot move from {} to {}",
                current.status.as_str(),
                status.as_str()
            );
        }
        let next = ScientificNode {
            status,
            updated_at,
            ..current
        };
        validate_scientific_node(&next).map_err(|error| anyhow::anyhow!(error))?;
        let updated = sqlx::query(
            "UPDATE scientific_cycle_nodes SET status=?, updated_at=? WHERE id=? AND status=?",
        )
        .bind(status.as_str())
        .bind(updated_at)
        .bind(id)
        .bind(current.status.as_str())
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() != 1 {
            bail!("scientific status changed before the update was stored");
        }
        Ok(next)
    }

    pub async fn insert_scientific_edge(&self, edge: &ScientificEdge) -> Result<()> {
        let mut conn = self.pool.acquire().await?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *conn)
            .await
            .context("begin scientific edge write")?;
        let result = insert_scientific_edge_locked(&mut conn, edge).await;
        let finish = if result.is_ok() {
            sqlx::query("COMMIT").execute(&mut *conn).await
        } else {
            sqlx::query("ROLLBACK").execute(&mut *conn).await
        };
        finish.context("finish scientific edge write")?;
        result
    }

    pub async fn list_scientific_edges(&self, cycle_id: &str) -> Result<Vec<ScientificEdge>> {
        let rows = sqlx::query(
            "SELECT id,cycle_id,from_id,to_id,kind,created_at \
             FROM scientific_cycle_edges WHERE cycle_id=? ORDER BY created_at,id",
        )
        .bind(cycle_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let kind = ScientificEdgeKind::parse(row.get::<String, _>(4).as_str())
                    .context("unknown scientific edge kind")?;
                Ok(ScientificEdge {
                    id: row.get(0),
                    cycle_id: row.get(1),
                    from_id: row.get(2),
                    to_id: row.get(3),
                    kind,
                    created_at: row.get(5),
                })
            })
            .collect()
    }
}

async fn insert_scientific_edge_locked(
    conn: &mut sqlx::SqliteConnection,
    edge: &ScientificEdge,
) -> Result<()> {
    let from = sqlx::query("SELECT cycle_id FROM scientific_cycle_nodes WHERE id=?")
        .bind(&edge.from_id)
        .fetch_optional(&mut *conn)
        .await?
        .context("scientific edge source is missing")?;
    let to = sqlx::query("SELECT cycle_id FROM scientific_cycle_nodes WHERE id=?")
        .bind(&edge.to_id)
        .fetch_optional(&mut *conn)
        .await?
        .context("scientific edge target is missing")?;
    let from_cycle: String = from.get(0);
    let to_cycle: String = to.get(0);
    if from_cycle != edge.cycle_id || to_cycle != edge.cycle_id {
        bail!("scientific edge endpoints must belong to the same cycle");
    }
    let rows = sqlx::query(
        "SELECT id,cycle_id,from_id,to_id,kind,created_at FROM scientific_cycle_edges WHERE cycle_id=?",
    )
    .bind(&edge.cycle_id)
    .fetch_all(&mut *conn)
    .await?;
    let mut edges = Vec::new();
    for row in rows {
        let kind = ScientificEdgeKind::parse(row.get::<String, _>(4).as_str())
            .context("unknown scientific edge kind")?;
        edges.push(ScientificEdge {
            id: row.get(0),
            cycle_id: row.get(1),
            from_id: row.get(2),
            to_id: row.get(3),
            kind,
            created_at: row.get(5),
        });
    }
    edges.push(edge.clone());
    validate_scientific_edges(&edges).map_err(|error| anyhow::anyhow!(error))?;
    sqlx::query(
        "INSERT INTO scientific_cycle_edges(id,cycle_id,from_id,to_id,kind,created_at) \
         VALUES(?,?,?,?,?,?)",
    )
    .bind(&edge.id)
    .bind(&edge.cycle_id)
    .bind(&edge.from_id)
    .bind(&edge.to_id)
    .bind(edge.kind.as_str())
    .bind(edge.created_at)
    .execute(&mut *conn)
    .await
    .context("insert scientific cycle edge")?;
    Ok(())
}

fn node_from_row(row: sqlx::sqlite::SqliteRow) -> Result<ScientificNode> {
    let kind = ScientificNodeKind::parse(row.get::<String, _>(4).as_str())
        .context("unknown scientific node kind")?;
    let status = ScientificNodeStatus::parse(row.get::<String, _>(5).as_str())
        .context("unknown scientific node status")?;
    let refs = serde_json::from_str(row.get::<String, _>(7).as_str())?;
    Ok(ScientificNode {
        id: row.get(0),
        cycle_id: row.get(1),
        project_id: row.get(2),
        frame_id: row.get(3),
        kind,
        status,
        payload_json: row.get(6),
        refs,
        created_at: row.get(8),
        updated_at: row.get(9),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wisp_dto::{ScientificRef, ScientificRefKind};

    fn node(
        id: &str,
        kind: ScientificNodeKind,
        status: ScientificNodeStatus,
        payload: &str,
        refs: Vec<ScientificRef>,
        at: i64,
    ) -> ScientificNode {
        ScientificNode {
            id: id.into(),
            cycle_id: "cycle-1".into(),
            project_id: "p".into(),
            frame_id: "frame-1".into(),
            kind,
            status,
            payload_json: payload.into(),
            refs,
            created_at: at,
            updated_at: at,
        }
    }

    #[tokio::test]
    async fn one_depmap_cycle_keeps_versions_and_rejects_a_loop() {
        let tmp = std::env::temp_dir().join(format!(
            "wisp_scientific_cycle_{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let store = Store::open(&tmp).await.unwrap();
        store.create_project("p", "proj", "").await.unwrap();
        let evidence = ScientificRef {
            kind: ScientificRefKind::Evidence,
            id: "ev-1".into(),
            version: "sha256:abc".into(),
        };
        store
            .insert_scientific_node(&node(
                "q",
                ScientificNodeKind::ResearchQuestion,
                ScientificNodeStatus::Active,
                r#"{"text":"which lineage depends on the target"}"#,
                vec![],
                1,
            ))
            .await
            .unwrap();
        store
            .insert_scientific_node(&node(
                "h",
                ScientificNodeKind::HypothesisCandidate,
                ScientificNodeStatus::Proposed,
                r#"{"text":"dependency is selective"}"#,
                vec![],
                2,
            ))
            .await
            .unwrap();
        store
            .insert_scientific_node(&node(
                "plan",
                ScientificNodeKind::AnalysisPlan,
                ScientificNodeStatus::Active,
                r#"{"query":"dependency"}"#,
                vec![],
                3,
            ))
            .await
            .unwrap();
        store
            .insert_scientific_node(&node(
                "obs",
                ScientificNodeKind::Observation,
                ScientificNodeStatus::Measured,
                r#"{"summary":"score recorded"}"#,
                vec![evidence],
                4,
            ))
            .await
            .unwrap();
        store
            .insert_scientific_node(&node(
                "decision",
                ScientificNodeKind::Decision,
                ScientificNodeStatus::Active,
                r#"{"choice":"follow the selective lineage"}"#,
                vec![],
                5,
            ))
            .await
            .unwrap();
        store
            .insert_scientific_node(&node(
                "next",
                ScientificNodeKind::FollowUpHypothesis,
                ScientificNodeStatus::Proposed,
                r#"{"text":"the selectivity is pathway-limited"}"#,
                vec![],
                6,
            ))
            .await
            .unwrap();
        for (index, (from, to)) in [
            ("q", "h"),
            ("h", "plan"),
            ("plan", "obs"),
            ("obs", "decision"),
            ("decision", "next"),
        ]
        .into_iter()
        .enumerate()
        {
            store
                .insert_scientific_edge(&ScientificEdge {
                    id: format!("e{index}"),
                    cycle_id: "cycle-1".into(),
                    from_id: from.into(),
                    to_id: to.into(),
                    kind: ScientificEdgeKind::Follows,
                    created_at: 10 + index as i64,
                })
                .await
                .unwrap();
        }
        let listed = store.list_scientific_cycle("cycle-1").await.unwrap();
        assert_eq!(listed.len(), 6);
        assert_eq!(listed[3].refs[0].version, "sha256:abc");
        let err = store
            .insert_scientific_edge(&ScientificEdge {
                id: "loop".into(),
                cycle_id: "cycle-1".into(),
                from_id: "next".into(),
                to_id: "q".into(),
                kind: ScientificEdgeKind::Follows,
                created_at: 20,
            })
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("acyclic"), "{err}");
        let measured = store
            .insert_scientific_node(&node(
                "bad-h",
                ScientificNodeKind::HypothesisCandidate,
                ScientificNodeStatus::Proposed,
                r#"{"measured_value":1}"#,
                vec![],
                7,
            ))
            .await
            .unwrap_err()
            .to_string();
        assert!(measured.contains("measured value"), "{measured}");
        let _ = std::fs::remove_file(tmp);
    }
}
