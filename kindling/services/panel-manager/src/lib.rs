// Copyright (c) 2024 Marceline Cramer
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// This file is part of Hearth.
//
// Hearth is free software: you can redistribute it and/or modify it under the
// terms of the GNU Affero General Public License as published by the Free
// Software Foundation, either version 3 of the License, or (at your option)
// any later version.
//
// Hearth is distributed in the hope that it will be useful, but WITHOUT ANY
// WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
// FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License for more
// details.
//
// You should have received a copy of the GNU Affero General Public License
// along with Hearth. If not, see <https://www.gnu.org/licenses/>.

use glam::{Vec2, Vec3};
use hearth_guest::{export_metadata, Capability, PARENT};
use kindling_host::prelude::*;
use kindling_schema::panel::*;

export_metadata!();

struct Panel {
    on_event: Capability,
    transform: PanelTransform,
    last_cursor_pos: Vec2,
}

impl Panel {
    fn event(&self, event: PanelEvent) {
        self.on_event.send(&event, &[]);
    }
}

#[derive(Default)]
struct PanelManager {
    panels: Vec<Panel>,
    focused_panel: Option<usize>,
    cursor: Option<Cursor>,
    last_redraw: Stopwatch,
}

impl PanelManager {
    fn on_request(&mut self, request: PanelManagerRequest, caps: Vec<Capability>) {
        match request {
            PanelManagerRequest::CreatePanel { transform } => {
                let on_event = caps.first().unwrap().to_owned();
                self.create_panel(transform, on_event);
            }
            PanelManagerRequest::UpdateCursor(cursor) => {
                self.update_cursor(cursor);
            }
            PanelManagerRequest::DisableCursor => self.disable_cursor(),
            PanelManagerRequest::Redraw => self.redraw(),
        }
    }

    fn create_panel(&mut self, transform: PanelTransform, on_event: Capability) {
        self.panels.push(Panel {
            on_event,
            transform,
            last_cursor_pos: Vec2::NAN,
        });

        // panels are dirtied so touch cursor in-place
        if let Some(cursor) = self.cursor.as_ref() {
            self.update_cursor(cursor.to_owned());
        }
    }

    fn update_cursor(&mut self, cursor: Cursor) {
        // calculate the closest cursor's panel intersection
        let Some((at, idx)) = self.raycast(cursor.clone()) else {
            // no panel hit, defocus current
            self.defocus_current();
            return;
        };

        // track whether the current panel has been entered
        let mut entered = false;

        // if focus has changed, we're focusing the current panel
        if Some(idx) != self.focused_panel {
            // attempt to defocus the previous focused panel
            self.defocus_current();
            entered = true;
        }

        // set the focused panel
        self.focused_panel = Some(idx);

        // retrieve a reference to the focused panel
        let panel = &mut self.panels[idx];

        // send focus event if entering
        if entered {
            panel.event(PanelEvent::FocusGained);
        }

        // retrieve the last mouse select state
        let last_select = self
            .cursor
            .as_ref()
            .map(|cursor| cursor.select)
            .unwrap_or(false);

        // update the latest cursor
        let select = cursor.select;
        self.cursor = Some(cursor);

        // send a cursor event with a kind based on the select state
        panel.event(PanelEvent::CursorEvent {
            at,
            kind: match (entered, last_select, select) {
                (true, _, _) => CursorEventKind::Entered(select),
                (_, false, true) => CursorEventKind::ClickDown,
                (_, true, false) => CursorEventKind::ClickUp,
                _ => {
                    // skip sending move event if cursor position is close enough
                    if (at - panel.last_cursor_pos).length_squared() < 0.0001 {
                        // bypass updating last_cursor_pos within dead zone
                        return;
                    }

                    CursorEventKind::Move
                }
            },
        });

        // update the panel's latest cursor position
        panel.last_cursor_pos = at;
    }

    fn raycast(&self, cursor: Cursor) -> Option<(Vec2, usize)> {
        let mut closest: Option<(f32, Vec2, usize)> = None;

        for (idx, panel) in self.panels.iter().enumerate() {
            // panel plane normal
            let n = panel.transform.orientation.mul_vec3(Vec3::Z);

            // distance from ray origin to plane
            let d = (panel.transform.position - cursor.origin).dot(n);

            // rate of distance change along ray direction
            let rd = cursor.dir.dot(n);

            // point along ray which hits panel plane
            let hit = d / rd;

            // skip backwards raycasts
            if hit < 0.0 || !hit.is_finite() {
                continue;
            }

            // transform from world space to panel space
            let inv_panel = panel.transform.orientation.inverse();

            // local space coords of collision
            let local_at = cursor.origin + cursor.dir * hit - panel.transform.position;

            // panel space coords of collision
            let at = inv_panel.mul_vec3(local_at).truncate();

            // discard intersections outside of the panel's bounds
            if at.abs().cmpge(panel.transform.half_size).any() {
                continue;
            }

            // bundle the intersection info and discard Z info
            let intersection = (hit, at, idx);

            // get the current closest (or set ours if there is none currently)
            let closest = closest.get_or_insert(intersection);

            // set a new closest if the distance is closer
            if intersection.0 < closest.0 {
                *closest = intersection;
            }
        }

        closest.map(|(_, at, idx)| (at, idx))
    }

    fn disable_cursor(&mut self) {
        if self.cursor.take().is_none() {
            return;
        }

        self.defocus_current();
    }

    fn redraw(&mut self) {
        let dt = self.last_redraw.lap();

        for panel in self.panels.iter() {
            panel.event(PanelEvent::Redraw(dt));
        }
    }

    fn defocus_current(&mut self) {
        let Some(old) = self.focused_panel.take() else {
            return;
        };

        let old = &self.panels[old];

        old.event(PanelEvent::FocusLost);

        old.event(PanelEvent::CursorEvent {
            at: old.last_cursor_pos,
            kind: CursorEventKind::Left,
        });
    }
}

#[no_mangle]
pub extern "C" fn run() {
    let mut app = PanelManager::default();

    loop {
        // TODO remove panels with downed on_events
        let (request, caps) = PARENT.recv();
        app.on_request(request, caps);
    }
}
