//! The chat with mouce: the conversation, how it turns into an AI request, and the
//! optional saved history (text only; screen images are never saved).

use crate::ai::{ChatMessage, ChatRequest, ImageInput, Role};
use crate::store::{AppStore, StoreError};
use serde::{Deserialize, Serialize};

pub const SYSTEM_PROMPT: &str = "You are mouce, a small, friendly helper that lives in a \
corner of the user's Windows desktop. When the user shares their screen you get a picture \
of it; otherwise you cannot see it. You can only look and explain: you cannot click, type, \
open, run or change anything on their computer, so never offer to. When something needs \
doing, tell them how to do it themselves. Keep answers short and clear, in plain text \
(short lists with \"-\" are fine).";

pub const MAX_TOKENS: u32 = 1024;
/// The question used when the user shares their screen without typing anything.
pub const LOOK_QUESTION: &str = "What's on my screen?";
/// The question used when the user sends a circled area without typing anything.
pub const CIRCLE_QUESTION: &str = "What is this?";
/// How many of the latest messages go to the AI with each question.
const SENT_MESSAGES: usize = 20;
/// How many messages the saved history keeps.
const SAVED_MESSAGES: usize = 100;
const HISTORY_KEY: &str = "chat_history";

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum TurnRole {
    User,
    Assistant,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Turn {
    pub role: TurnRole,
    pub text: String,
    /// The user shared their screen with this message.
    #[serde(default)]
    pub looked: bool,
    /// The user circled part of their screen for this message.
    #[serde(default)]
    pub circled: bool,
}

/// A circled part of the screen waiting to be asked about: the image for the AI and a
/// small preview for the chat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snip {
    pub image: ImageInput,
    pub thumbnail: String,
}

/// The conversation in memory. Only the most recent screen image is kept, attached to
/// the message it was shared with, so follow-up questions can still refer to it.
#[derive(Default, Debug)]
pub struct Conversation {
    turns: Vec<Turn>,
    screen: Option<(usize, ImageInput)>,
}

impl Conversation {
    pub fn from_turns(turns: Vec<Turn>) -> Conversation {
        Conversation {
            turns,
            screen: None,
        }
    }

    pub fn turns(&self) -> &[Turn] {
        &self.turns
    }

    pub fn push_user(&mut self, text: String, screen: Option<ImageInput>) {
        let looked = screen.is_some();
        if let Some(image) = screen {
            self.screen = Some((self.turns.len(), image));
        }
        self.turns.push(Turn {
            role: TurnRole::User,
            text,
            looked,
            circled: false,
        });
    }

    /// Adds a question about a circled part of the screen.
    pub fn push_user_circled(&mut self, text: String, image: ImageInput) {
        self.screen = Some((self.turns.len(), image));
        self.turns.push(Turn {
            role: TurnRole::User,
            text,
            looked: false,
            circled: true,
        });
    }

    pub fn push_assistant(&mut self, text: String) {
        self.turns.push(Turn {
            role: TurnRole::Assistant,
            text,
            looked: false,
            circled: false,
        });
    }

    /// Takes back the last question when it got no answer, so it can be asked again.
    pub fn drop_unanswered(&mut self) {
        if self.turns.last().is_some_and(|t| t.role == TurnRole::User) {
            self.turns.pop();
            if self
                .screen
                .as_ref()
                .is_some_and(|(i, _)| *i == self.turns.len())
            {
                self.screen = None;
            }
        }
    }

    pub fn clear(&mut self) {
        self.turns.clear();
        self.screen = None;
    }

    /// The request for the latest messages. It always starts with a user message, as
    /// every provider expects.
    pub fn request(&self, model: String) -> ChatRequest {
        let mut start = self.turns.len().saturating_sub(SENT_MESSAGES);
        while self
            .turns
            .get(start)
            .is_some_and(|t| t.role != TurnRole::User)
        {
            start += 1;
        }
        let messages = self.turns[start..]
            .iter()
            .enumerate()
            .map(|(offset, t)| ChatMessage {
                role: match t.role {
                    TurnRole::User => Role::User,
                    TurnRole::Assistant => Role::Assistant,
                },
                text: t.text.clone(),
                images: match &self.screen {
                    Some((i, image)) if *i == start + offset => vec![image.clone()],
                    _ => Vec::new(),
                },
            })
            .collect();
        ChatRequest {
            model,
            system: Some(SYSTEM_PROMPT.to_string()),
            messages,
            max_tokens: MAX_TOKENS,
        }
    }
}

/// The saved history; missing or unreadable history is an empty one.
pub fn load_history(store: &AppStore) -> Vec<Turn> {
    store
        .get_kv(HISTORY_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Saves the latest `SAVED_MESSAGES` messages (text only).
pub fn save_history(store: &AppStore, turns: &[Turn]) -> Result<(), StoreError> {
    let keep = &turns[turns.len().saturating_sub(SAVED_MESSAGES)..];
    let raw = serde_json::to_string(keep).map_err(|e| StoreError::Io(e.to_string()))?;
    store.set_kv(HISTORY_KEY, &raw)
}

pub fn forget_history(store: &AppStore) -> Result<(), StoreError> {
    store.delete_kv(HISTORY_KEY)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> ImageInput {
        ImageInput {
            mime: "image/jpeg".into(),
            base64: "QUJD".into(),
        }
    }

    fn store() -> (tempfile::TempDir, AppStore) {
        let dir = tempfile::tempdir().unwrap();
        let s = AppStore::open(dir.path().to_path_buf()).unwrap();
        (dir, s)
    }

    #[test]
    fn request_has_system_prompt_and_every_message() {
        let mut c = Conversation::default();
        c.push_user("hi".into(), None);
        c.push_assistant("hello".into());
        c.push_user("what now?".into(), None);
        let req = c.request("m".into());
        assert_eq!(req.model, "m");
        assert_eq!(req.system.as_deref(), Some(SYSTEM_PROMPT));
        assert_eq!(req.max_tokens, MAX_TOKENS);
        let texts: Vec<_> = req
            .messages
            .iter()
            .map(|m| (m.role, m.text.as_str()))
            .collect();
        assert_eq!(
            texts,
            vec![
                (Role::User, "hi"),
                (Role::Assistant, "hello"),
                (Role::User, "what now?")
            ]
        );
    }

    #[test]
    fn only_the_latest_screen_image_is_sent_with_its_own_message() {
        let mut c = Conversation::default();
        c.push_user("look 1".into(), Some(image()));
        c.push_assistant("a".into());
        c.push_user("look 2".into(), Some(image()));
        c.push_assistant("b".into());
        c.push_user("and that button?".into(), None);
        let req = c.request("m".into());
        let with_images: Vec<_> = req
            .messages
            .iter()
            .filter(|m| !m.images.is_empty())
            .map(|m| &m.text)
            .collect();
        assert_eq!(with_images, vec!["look 2"]);
        assert!(c.turns()[0].looked && c.turns()[2].looked && !c.turns()[4].looked);
    }

    #[test]
    fn long_conversations_send_the_latest_messages_starting_with_the_user() {
        let mut c = Conversation::default();
        for i in 0..15 {
            c.push_user(format!("q{i}"), None);
            c.push_assistant(format!("a{i}"));
        }
        c.push_user("last".into(), None);
        let req = c.request("m".into());
        assert!(req.messages.len() <= SENT_MESSAGES);
        assert_eq!(req.messages[0].role, Role::User);
        assert_eq!(req.messages.last().unwrap().text, "last");
    }

    #[test]
    fn an_image_that_scrolled_out_of_the_window_is_not_sent() {
        let mut c = Conversation::default();
        c.push_user("look".into(), Some(image()));
        for i in 0..15 {
            c.push_assistant(format!("a{i}"));
            c.push_user(format!("q{i}"), None);
        }
        assert!(c
            .request("m".into())
            .messages
            .iter()
            .all(|m| m.images.is_empty()));
    }

    #[test]
    fn drop_unanswered_removes_the_question_and_its_image() {
        let mut c = Conversation::default();
        c.push_user("hi".into(), None);
        c.push_assistant("hello".into());
        c.push_user("look".into(), Some(image()));
        c.drop_unanswered();
        assert_eq!(c.turns().len(), 2);
        assert!(c
            .request("m".into())
            .messages
            .iter()
            .all(|m| m.images.is_empty()));
        c.drop_unanswered();
        assert_eq!(c.turns().len(), 2, "an answered question stays");
    }

    #[test]
    fn a_circled_image_is_sent_with_its_question_and_remembered_as_circled() {
        let mut c = Conversation::default();
        c.push_user_circled("what is this?".into(), image());
        c.push_assistant("a chart".into());
        c.push_user("and the red line?".into(), None);
        let req = c.request("m".into());
        assert_eq!(req.messages[0].images, vec![image()]);
        assert!(req.messages[2].images.is_empty());
        assert!(c.turns()[0].circled && !c.turns()[0].looked);
        c.drop_unanswered();
        c.drop_unanswered();
        assert_eq!(c.turns().len(), 2, "answered questions stay");
    }

    #[test]
    fn history_saved_before_circling_existed_still_loads() {
        let old = r#"[{"role":"user","text":"hi","looked":true}]"#;
        let turns: Vec<Turn> = serde_json::from_str(old).unwrap();
        assert!(turns[0].looked && !turns[0].circled);
    }

    #[test]
    fn clear_forgets_everything() {
        let mut c = Conversation::default();
        c.push_user("look".into(), Some(image()));
        c.clear();
        assert!(c.turns().is_empty());
        c.push_user("hi".into(), None);
        assert!(c.request("m".into()).messages[0].images.is_empty());
    }

    #[test]
    fn history_round_trips_text_only_and_keeps_the_latest() {
        let (_d, s) = store();
        assert!(load_history(&s).is_empty());
        let mut c = Conversation::default();
        for i in 0..60 {
            c.push_user(format!("q{i}"), (i == 59).then(image));
            c.push_assistant(format!("a{i}"));
        }
        save_history(&s, c.turns()).unwrap();
        let loaded = load_history(&s);
        assert_eq!(loaded.len(), SAVED_MESSAGES);
        assert_eq!(loaded.last().unwrap().text, "a59");
        assert!(loaded[loaded.len() - 2].looked);
        let raw = s.get_kv(HISTORY_KEY).unwrap().unwrap();
        assert!(!raw.contains("QUJD"), "screen images are never saved");

        forget_history(&s).unwrap();
        assert!(load_history(&s).is_empty());
        assert_eq!(s.get_kv(HISTORY_KEY).unwrap(), None);
    }

    #[test]
    fn unreadable_history_is_empty() {
        let (_d, s) = store();
        s.set_kv(HISTORY_KEY, "not json").unwrap();
        assert!(load_history(&s).is_empty());
    }
}
