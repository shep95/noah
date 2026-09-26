//! The conversation tree: chats grouped by recency, with branches, side
//! chats and hand-offs nested under the conversation they came from.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use chrono::{DateTime, TimeZone, Utc};

use crate::MAX_BRANCH_DEPTH;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConversationKind {
    /// A new chat.
    Root,
    /// Split from a message in its parent, inheriting everything before it.
    Branch,
    /// Opened from somewhere else, such as a shepherd-room thread, to
    /// discuss it without filling that thread's context.
    Side,
    /// The shepherd-room thread a chat was taken to.
    Handoff,
}

impl ConversationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ConversationKind::Root => "root",
            ConversationKind::Branch => "branch",
            ConversationKind::Side => "side",
            ConversationKind::Handoff => "handoff",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "root" => Some(ConversationKind::Root),
            "branch" => Some(ConversationKind::Branch),
            "side" => Some(ConversationKind::Side),
            "handoff" => Some(ConversationKind::Handoff),
            _ => None,
        }
    }

    pub fn glyph(self) -> Option<&'static str> {
        match self {
            ConversationKind::Root => None,
            ConversationKind::Branch => Some("↳"),
            ConversationKind::Side => Some("~"),
            ConversationKind::Handoff => Some("→"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConversationNode<Id> {
    pub id: Id,
    pub parent: Option<Id>,
    pub kind: ConversationKind,
    pub title: String,
    pub agent: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecencyGroup {
    Today,
    ThisWeek,
    Older,
}

impl RecencyGroup {
    pub fn label(self) -> &'static str {
        match self {
            RecencyGroup::Today => "today",
            RecencyGroup::ThisWeek => "this week",
            RecencyGroup::Older => "older",
        }
    }
}

/// Which heading a conversation last touched at `updated_at` sits under, as
/// seen at `now` in the person's time zone.
pub fn recency_group<Tz: TimeZone>(updated_at: &DateTime<Utc>, now: &DateTime<Tz>) -> RecencyGroup {
    let updated_day = updated_at.with_timezone(&now.timezone()).date_naive();
    let today = now.date_naive();
    if updated_day >= today {
        RecencyGroup::Today
    } else if (today - updated_day).num_days() < 7 {
        RecencyGroup::ThisWeek
    } else {
        RecencyGroup::Older
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TreeRow<Id> {
    Group(RecencyGroup),
    Conversation {
        id: Id,
        /// 0 for a top-level conversation.
        depth: usize,
        kind: ConversationKind,
        title: String,
        agent: Option<String>,
    },
}

/// Lays the conversations out as rows: a heading per recency group, each
/// top-level conversation under the group of its most recent activity
/// (including its branches'), newest first, with its descendants below it
/// oldest first. Conversations whose parent is gone, or whose parent links
/// loop or run too deep, show at the top level. With a `query`, only
/// conversations whose title or agent contains it are kept, along with their
/// ancestors so each match shows where it sits.
pub fn build_tree<Id, Tz>(
    nodes: Vec<ConversationNode<Id>>,
    now: &DateTime<Tz>,
    query: Option<&str>,
) -> Vec<TreeRow<Id>>
where
    Id: Clone + Eq + Hash,
    Tz: TimeZone,
{
    let parents: HashMap<Id, Id> = nodes
        .iter()
        .filter_map(|node| Some((node.id.clone(), node.parent.clone()?)))
        .collect();
    let known: HashSet<Id> = nodes.iter().map(|node| node.id.clone()).collect();
    let attached_parent = |node: &ConversationNode<Id>| -> Option<Id> {
        let parent = node.parent.clone()?;
        let reachable = known.contains(&parent)
            && crate::ancestors(&node.id, |id| {
                parents
                    .get(id)
                    .filter(|parent| known.contains(*parent))
                    .cloned()
            })
            .is_ok();
        reachable.then_some(parent)
    };

    let mut children: HashMap<Id, Vec<usize>> = HashMap::new();
    let mut roots = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        match attached_parent(node) {
            Some(parent) => children.entry(parent).or_default().push(index),
            None => roots.push(index),
        }
    }
    for siblings in children.values_mut() {
        siblings.sort_by_key(|index| nodes.get(*index).map(|node| node.created_at));
    }

    let query = query
        .map(|query| query.trim().to_lowercase())
        .filter(|query| !query.is_empty());
    let mut kept = HashSet::new();
    let mut latest = HashMap::new();
    for root in &roots {
        mark_subtree(
            *root,
            &nodes,
            &children,
            query.as_deref(),
            0,
            &mut kept,
            &mut latest,
        );
    }

    let mut ordered_roots: Vec<usize> = roots
        .into_iter()
        .filter(|index| kept.contains(index))
        .collect();
    ordered_roots.sort_by(|left, right| latest.get(right).cmp(&latest.get(left)));

    let mut rows = Vec::new();
    let mut current_group = None;
    for root in ordered_roots {
        let Some(root_latest) = latest.get(&root) else {
            continue;
        };
        let group = recency_group(root_latest, now);
        if current_group != Some(group) {
            rows.push(TreeRow::Group(group));
            current_group = Some(group);
        }
        push_rows(root, 0, &nodes, &children, &kept, &mut rows);
    }
    rows
}

/// Whether the subtree at `index` has anything the query keeps, recording
/// kept nodes and each node's latest activity across its subtree.
fn mark_subtree<Id: Clone + Eq + Hash>(
    index: usize,
    nodes: &[ConversationNode<Id>],
    children: &HashMap<Id, Vec<usize>>,
    query: Option<&str>,
    depth: usize,
    kept: &mut HashSet<usize>,
    latest: &mut HashMap<usize, DateTime<Utc>>,
) -> bool {
    let Some(node) = nodes.get(index) else {
        return false;
    };
    let matches = query.is_none_or(|query| {
        node.title.to_lowercase().contains(query)
            || node
                .agent
                .as_ref()
                .is_some_and(|agent| agent.to_lowercase().contains(query))
    });
    let mut keep = matches;
    let mut subtree_latest = node.updated_at;
    if depth < MAX_BRANCH_DEPTH
        && let Some(child_indices) = children.get(&node.id)
    {
        for child in child_indices {
            if mark_subtree(*child, nodes, children, query, depth + 1, kept, latest) {
                keep = true;
            }
            if let Some(child_latest) = latest.get(child) {
                subtree_latest = subtree_latest.max(*child_latest);
            }
        }
    }
    latest.insert(index, subtree_latest);
    if keep {
        kept.insert(index);
    }
    keep
}

fn push_rows<Id: Clone + Eq + Hash>(
    index: usize,
    depth: usize,
    nodes: &[ConversationNode<Id>],
    children: &HashMap<Id, Vec<usize>>,
    kept: &HashSet<usize>,
    rows: &mut Vec<TreeRow<Id>>,
) {
    let Some(node) = nodes.get(index) else {
        return;
    };
    if !kept.contains(&index) {
        return;
    }
    rows.push(TreeRow::Conversation {
        id: node.id.clone(),
        depth,
        kind: node.kind,
        title: node.title.clone(),
        agent: node.agent.clone(),
    });
    if depth >= MAX_BRANCH_DEPTH {
        return;
    }
    if let Some(child_indices) = children.get(&node.id) {
        for child in child_indices {
            push_rows(*child, depth + 1, nodes, children, kept, rows);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    fn node(
        id: &'static str,
        parent: Option<&'static str>,
        kind: ConversationKind,
        hours_ago: i64,
        now: DateTime<Utc>,
    ) -> ConversationNode<&'static str> {
        ConversationNode {
            id,
            parent,
            kind,
            title: id.replace('_', " "),
            agent: None,
            created_at: now - Duration::hours(hours_ago),
            updated_at: now - Duration::hours(hours_ago),
        }
    }

    fn labels(rows: &[TreeRow<&'static str>]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                TreeRow::Group(group) => format!("# {}", group.label()),
                TreeRow::Conversation {
                    depth, kind, title, ..
                } => format!(
                    "{}{}{}",
                    "  ".repeat(*depth),
                    kind.glyph()
                        .map(|glyph| format!("{glyph} "))
                        .unwrap_or_default(),
                    title
                ),
            })
            .collect()
    }

    #[test]
    fn groups_and_nests_conversations() {
        let now = Utc
            .with_ymd_and_hms(2026, 9, 25, 18, 0, 0)
            .single()
            .expect("valid time");
        use ConversationKind::*;
        let nodes = vec![
            node("pricing_page_draft", None, Root, 50, now),
            node("webhook_retries", None, Root, 3, now),
            node("with_redis", Some("webhook_retries"), Branch, 2, now),
            node("redis_streams_instead", Some("with_redis"), Branch, 1, now),
            node(
                "opus_vs_qwen",
                Some("webhook_retries"),
                Branch,
                2 * 24 + 1,
                now,
            ),
            node(
                "handed_to_code",
                Some("pricing_page_draft"),
                Handoff,
                49,
                now,
            ),
            node("auth_question", Some("missing_parent"), Side, 60, now),
            node("old_notes", None, Root, 24 * 30, now),
        ];
        let rows = build_tree(nodes, &now, None);
        assert_eq!(
            labels(&rows),
            [
                "# today",
                "webhook retries",
                "  ↳ opus vs qwen",
                "  ↳ with redis",
                "    ↳ redis streams instead",
                "# this week",
                "pricing page draft",
                "  → handed to code",
                "~ auth question",
                "# older",
                "old notes",
            ]
        );
    }

    #[test]
    fn a_recent_branch_lifts_its_root_into_today() {
        let now = Utc
            .with_ymd_and_hms(2026, 9, 25, 18, 0, 0)
            .single()
            .expect("valid time");
        let nodes = vec![
            node("old_root", None, ConversationKind::Root, 24 * 20, now),
            node(
                "fresh_branch",
                Some("old_root"),
                ConversationKind::Branch,
                1,
                now,
            ),
        ];
        assert_eq!(
            labels(&build_tree(nodes, &now, None)),
            ["# today", "old root", "  ↳ fresh branch"]
        );
    }

    #[test]
    fn search_keeps_ancestors_of_matches() {
        let now = Utc
            .with_ymd_and_hms(2026, 9, 25, 18, 0, 0)
            .single()
            .expect("valid time");
        use ConversationKind::*;
        let nodes = vec![
            node("webhook_retries", None, Root, 3, now),
            node("with_redis", Some("webhook_retries"), Branch, 2, now),
            node("redis_streams", Some("with_redis"), Branch, 1, now),
            node("pricing", None, Root, 4, now),
        ];
        assert_eq!(
            labels(&build_tree(nodes.clone(), &now, Some("STREAMS"))),
            [
                "# today",
                "webhook retries",
                "  ↳ with redis",
                "    ↳ redis streams"
            ]
        );
        assert!(build_tree(nodes, &now, Some("nothing like this")).is_empty());
    }

    #[test]
    fn looping_parents_still_show() {
        let now = Utc
            .with_ymd_and_hms(2026, 9, 25, 18, 0, 0)
            .single()
            .expect("valid time");
        let nodes = vec![
            node("a", Some("b"), ConversationKind::Branch, 1, now),
            node("b", Some("a"), ConversationKind::Branch, 2, now),
        ];
        assert_eq!(
            labels(&build_tree(nodes, &now, None)),
            ["# today", "↳ a", "↳ b"]
        );
    }

    #[test]
    fn groups_by_local_day() {
        let now = Utc
            .with_ymd_and_hms(2026, 9, 25, 1, 0, 0)
            .single()
            .expect("valid time");
        let yesterday_late = now - Duration::hours(2);
        assert_eq!(recency_group(&yesterday_late, &now), RecencyGroup::ThisWeek);
        assert_eq!(recency_group(&now, &now), RecencyGroup::Today);
        assert_eq!(
            recency_group(&(now - Duration::days(8)), &now),
            RecencyGroup::Older
        );
    }
}
