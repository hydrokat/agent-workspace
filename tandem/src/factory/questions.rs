use serde::{Deserialize, Serialize};

use crate::coordination::schema::Query;

pub const OPEN_QUESTION_LIMIT: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdeaQuestionItem {
    pub id: String,
    pub question: String,
    pub status: String,
    pub blocking: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdeaQuestions {
    pub open: usize,
    pub blocking: usize,
    pub items: Vec<IdeaQuestionItem>,
}

impl IdeaQuestions {
    pub fn at_limit(&self) -> bool {
        self.open >= OPEN_QUESTION_LIMIT
    }
}

pub fn for_idea(queries: &[Query], idea_id: &str) -> IdeaQuestions {
    let prefix = format!("idea:{idea_id}");
    let mut open = 0usize;
    let mut blocking = 0usize;
    let mut items = Vec::new();

    for q in queries {
        if q.task != prefix {
            continue;
        }
        let is_open = q.status == "open";
        let is_blocking = is_open && !q.blocking_scope.trim().is_empty();
        if is_open {
            open += 1;
        }
        if is_blocking {
            blocking += 1;
        }
        items.push(IdeaQuestionItem {
            id: q.id.clone(),
            question: q.question.clone(),
            status: q.status.clone(),
            blocking: is_blocking,
        });
    }

    IdeaQuestions {
        open,
        blocking,
        items,
    }
}
