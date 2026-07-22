// SPDX-License-Identifier: GPL-3.0-only

use std::{cell::RefCell, sync::Mutex};

use smithay::{
    backend::renderer::{
        ContextId,
        damage::OutputDamageTracker,
        gles::{GlesRenderbuffer, GlesTexture},
    },
    output::Output,
    wayland::image_copy_capture::{
        CursorSession, CursorSessionRef, Frame, FrameRef, Session, SessionRef,
    },
};

use crate::shell::{CosmicSurface, Workspace};

type ImageCopySessionsData = RefCell<ImageCopySessions>;
type PendingImageCopyBuffers = Mutex<Vec<(SessionRef, Frame)>>;

pub type SessionData = Mutex<SessionUserData>;

pub struct SessionUserData {
    pub dt: OutputDamageTracker,
    pub offscreen: Option<(ContextId<GlesTexture>, GlesRenderbuffer)>,
}

impl SessionUserData {
    pub fn new(tracker: OutputDamageTracker) -> SessionUserData {
        SessionUserData {
            dt: tracker,
            offscreen: None,
        }
    }
}

#[derive(Debug, Default)]
pub struct ImageCopySessions {
    sessions: Vec<Session>,
    cursor_sessions: Vec<CursorSession>,
}

pub trait SessionHolder {
    fn add_session(&mut self, session: Session);
    fn remove_session(&mut self, session: &SessionRef);
    fn has_sessions(&self) -> bool;

    fn add_cursor_session(&mut self, session: CursorSession);
    fn remove_cursor_session(&mut self, session: &CursorSessionRef);
    fn has_cursor_sessions(&self) -> bool;
    /// Visit sessions without allocating a temporary collection or cloning protocol references.
    fn for_each_cursor_session(&self, f: impl FnMut(&CursorSessionRef));
}

pub trait FrameHolder {
    fn add_frame(&mut self, session: SessionRef, frame: Frame);
    fn remove_frame(&mut self, frame: &FrameRef);
    fn remove_session_frames(&mut self, session: &SessionRef);
    fn take_pending_frames(&self) -> Vec<(SessionRef, Frame)>;
}

impl SessionHolder for Output {
    fn add_session(&mut self, session: Session) {
        self.user_data()
            .insert_if_missing(ImageCopySessionsData::default);
        self.user_data()
            .get::<ImageCopySessionsData>()
            .unwrap()
            .borrow_mut()
            .sessions
            .push(session);
    }

    fn remove_session(&mut self, session: &SessionRef) {
        self.user_data()
            .get::<ImageCopySessionsData>()
            .unwrap()
            .borrow_mut()
            .sessions
            .retain(|s| s != session);
    }

    fn has_sessions(&self) -> bool {
        self.user_data()
            .get::<ImageCopySessionsData>()
            .is_some_and(|sessions| !sessions.borrow().sessions.is_empty())
    }

    fn add_cursor_session(&mut self, session: CursorSession) {
        self.user_data()
            .insert_if_missing(ImageCopySessionsData::default);
        self.user_data()
            .get::<ImageCopySessionsData>()
            .unwrap()
            .borrow_mut()
            .cursor_sessions
            .push(session);
    }

    fn remove_cursor_session(&mut self, session: &CursorSessionRef) {
        self.user_data()
            .get::<ImageCopySessionsData>()
            .unwrap()
            .borrow_mut()
            .cursor_sessions
            .retain(|s| s != session);
    }

    fn has_cursor_sessions(&self) -> bool {
        self.user_data()
            .get::<ImageCopySessionsData>()
            .is_some_and(|sessions| !sessions.borrow().cursor_sessions.is_empty())
    }

    fn for_each_cursor_session(&self, mut f: impl FnMut(&CursorSessionRef)) {
        if let Some(sessions) = self.user_data().get::<ImageCopySessionsData>() {
            for session in &sessions.borrow().cursor_sessions {
                f(session);
            }
        }
    }
}

impl FrameHolder for Output {
    fn add_frame(&mut self, session: SessionRef, frame: Frame) {
        self.user_data()
            .insert_if_missing_threadsafe(PendingImageCopyBuffers::default);
        let mut pending = self
            .user_data()
            .get::<PendingImageCopyBuffers>()
            .unwrap()
            .lock()
            .unwrap();
        pending.retain(|(pending_session, _)| pending_session != &session);
        pending.push((session, frame));
    }
    fn remove_frame(&mut self, frame: &FrameRef) {
        if let Some(pending) = self.user_data().get::<PendingImageCopyBuffers>() {
            pending.lock().unwrap().retain(|(_, f)| f != frame);
        }
    }
    fn remove_session_frames(&mut self, session: &SessionRef) {
        if let Some(pending) = self.user_data().get::<PendingImageCopyBuffers>() {
            pending
                .lock()
                .unwrap()
                .retain(|(pending_session, _)| pending_session != session);
        }
    }
    fn take_pending_frames(&self) -> Vec<(SessionRef, Frame)> {
        self.user_data()
            .get::<PendingImageCopyBuffers>()
            .map(|pending| std::mem::take(&mut *pending.lock().unwrap()))
            .unwrap_or_default()
    }
}

impl SessionHolder for Workspace {
    fn add_session(&mut self, session: Session) {
        self.image_copy.sessions.push(session);
    }

    fn remove_session(&mut self, session: &SessionRef) {
        self.image_copy.sessions.retain(|s| s != session);
    }
    fn has_sessions(&self) -> bool {
        !self.image_copy.sessions.is_empty()
    }

    fn add_cursor_session(&mut self, session: CursorSession) {
        self.image_copy.cursor_sessions.push(session);
    }

    fn remove_cursor_session(&mut self, session: &CursorSessionRef) {
        self.image_copy.cursor_sessions.retain(|s| s != session);
    }
    fn has_cursor_sessions(&self) -> bool {
        !self.image_copy.cursor_sessions.is_empty()
    }

    fn for_each_cursor_session(&self, mut f: impl FnMut(&CursorSessionRef)) {
        for session in &self.image_copy.cursor_sessions {
            f(session);
        }
    }
}

impl SessionHolder for CosmicSurface {
    fn add_session(&mut self, session: Session) {
        self.user_data()
            .insert_if_missing(ImageCopySessionsData::default);
        self.user_data()
            .get::<ImageCopySessionsData>()
            .unwrap()
            .borrow_mut()
            .sessions
            .push(session);
    }

    fn remove_session(&mut self, session: &SessionRef) {
        self.user_data()
            .get::<ImageCopySessionsData>()
            .unwrap()
            .borrow_mut()
            .sessions
            .retain(|s| s != session);
    }
    fn has_sessions(&self) -> bool {
        self.user_data()
            .get::<ImageCopySessionsData>()
            .is_some_and(|sessions| !sessions.borrow().sessions.is_empty())
    }

    fn add_cursor_session(&mut self, session: CursorSession) {
        self.user_data()
            .insert_if_missing(ImageCopySessionsData::default);
        self.user_data()
            .get::<ImageCopySessionsData>()
            .unwrap()
            .borrow_mut()
            .cursor_sessions
            .push(session);
    }

    fn remove_cursor_session(&mut self, session: &CursorSessionRef) {
        self.user_data()
            .get::<ImageCopySessionsData>()
            .unwrap()
            .borrow_mut()
            .cursor_sessions
            .retain(|s| s != session);
    }

    fn has_cursor_sessions(&self) -> bool {
        self.user_data()
            .get::<ImageCopySessionsData>()
            .is_some_and(|sessions| !sessions.borrow().cursor_sessions.is_empty())
    }

    fn for_each_cursor_session(&self, mut f: impl FnMut(&CursorSessionRef)) {
        if let Some(sessions) = self.user_data().get::<ImageCopySessionsData>() {
            for session in &sessions.borrow().cursor_sessions {
                f(session);
            }
        }
    }
}
