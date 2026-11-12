//! Books (`BookEditScreen`, `BookSignScreen`, `BookViewScreen`,
//! `LecternScreen`): writing pages of a book and quill (14 lines of 114
//! pixels, 1024 characters, 100 pages), signing it into a written book,
//! reading written books, and lecterns (placing a book, turning its pages
//! with a redstone pulse, the comparator output and taking the book).
use super::{Game, chat::PLAYER_NAME, lan::LanHost, screen_info};
use horizon_client::{
    block_entity_item,
    containers::{BlockEntity, Lectern},
    first_person::Hand,
    hud::{HudBatch, rgba},
    item_data::{StackData, WrittenBook},
    items::ItemStack,
    menu::{Kind, Menu},
    mobs::Who,
    text::Text,
    ui,
    widgets::{self, Narratable, ScreenInfo},
};
use horizon_core::position::BlockPos;
use horizon_protocol::packets::play::{self, Clientbound as C, Serverbound as S};
use horizon_world::{level::flags as level_flags, nbt::Tag, registry::BlockStateId};
use winit::{
    event::{ElementState, KeyEvent},
    keyboard::{KeyCode, PhysicalKey},
};

const BLACK: u32 = 0xFF00_0000;
const PAGE_CHARS: usize = 1024;
const MAX_PAGES: usize = 100;
const LINES: usize = 14;

fn book_title_box() -> ui::EditBox {
    let mut edit = ui::EditBox::new(0.0, 0.0, 114.0, 9.0);
    edit.bordered = false;
    edit.max_length = 15;
    edit.text_color = BLACK;
    // BookSignScreen sets the title box centred.
    edit.centered = true;
    edit.focused = true;
    edit
}
fn book_page_box() -> ui::EditBox {
    let mut edit = ui::EditBox::new(0.0, 0.0, 114.0, 9.0);
    edit.bordered = false;
    edit.multiline = true;
    edit.max_length = PAGE_CHARS;
    edit.text_color = BLACK;
    edit.focused = true;
    edit
}

/// Where a book's text is, from its top left corner, above its page buttons.
const BOOK_TEXT: [f32; 4] = [36.0, 16.0, 120.0, 136.0];

pub(super) enum BookScreen {
    Edit {
        /// Inventory slot of the book (the selected hotbar slot).
        slot: usize,
        pages: Vec<String>,
        page: usize,
        signing: bool,
        title: String,
        /// BookSignScreen's title box (`EditBox` of 15).
        title_edit: ui::EditBox,
        /// BookEditScreen's page (`MultiLineEditBox`).
        page_edit: ui::EditBox,
    },
    View {
        pages: Vec<Text>,
        page: usize,
        lectern: Option<(i32, i32, i32)>,
    },
}
impl BookScreen {
    /// `BookSignScreen`'s constructor: its title box opens with the title
    /// kept from the last time, which Cancel stored (`titleValue`).
    fn begin_signing(&mut self) {
        if let Self::Edit {
            signing,
            title,
            title_edit,
            ..
        } = self
        {
            *signing = true;
            title_edit.set_value(title.as_str());
            title_edit.reset_blink();
        }
    }
}
/// The pages a book screen reads: a written book's components (so their
/// click events stay), else a writable book's text.
fn view_pages(data: Option<&StackData>) -> Vec<Text> {
    let Some(data) = data else { return Vec::new() };
    let Some(w) = &data.written else {
        return data.writable_pages.iter().map(Text::literal).collect();
    };
    let tagged = w.page_tags.len() == w.pages.len();
    w.pages
        .iter()
        .enumerate()
        .map(|(i, page)| match tagged.then(|| &w.page_tags[i]) {
            // A component object, or a component list (`ComponentSerialization`
            // takes both; servers behind ViaVersion send the list form).
            Some(
                tag @ (horizon_world::nbt::Tag::Compound(_) | horizon_world::nbt::Tag::List(_)),
            ) => horizon_client::entity_nbt::name_text(tag),
            _ => Text::literal(page),
        })
        .collect()
}
impl Game<'_> {
    fn book_lines(&self, page: &str) -> Vec<Text> {
        self.page_lines(&Text::literal(page))
    }
    fn page_lines(&self, page: &Text) -> Vec<Text> {
        self.gui.atlas.font.split(page, 114.0)
    }
    /// `BookViewScreen.mouseClicked`: the click event of the text under the
    /// cursor on the page.
    fn book_link_at(&self, page: &Text, left: f32) -> Option<horizon_client::text::ClickEvent> {
        self.book_style_at(page, left)?.click
    }
    /// `ActiveTextCollector` over the page: the style of the character
    /// under the cursor, for its click and hover events.
    fn book_style_at(&self, page: &Text, left: f32) -> Option<horizon_client::text::Style> {
        let (mx, my) = self.cursor;
        let row = ((my - 32.0) / 9.0).floor();
        if row < 0.0 || row >= LINES as f32 {
            return None;
        }
        let lines = self.page_lines(page);
        let line = lines.get(row as usize)?.resolve_legacy();
        let mut x = mx - (left + 36.0);
        if x < 0.0 {
            return None;
        }
        for (c, style) in line.chars() {
            x -= self.gui.atlas.font.advance(c, style);
            if x < 0.0 {
                return Some(style.clone());
            }
        }
        None
    }
    /// `BookViewScreen.handleClickEvent`: a page link turns to its page
    /// (`forcePage`), a web link goes through the confirm-link flow.
    fn book_follow_link(
        &mut self,
        link: horizon_client::text::ClickEvent,
        count: usize,
        lectern: Option<(i32, i32, i32)>,
    ) {
        use horizon_client::text::ClickEvent;
        match link {
            ClickEvent::ChangePage(target) => {
                let target = (target - 1).clamp(0, count.saturating_sub(1) as i32);
                let Some(BookScreen::View { page, .. }) = self.book.as_mut() else {
                    return;
                };
                if let Some(key) = lectern {
                    // LecternScreen.forcePage: the menu's button 100 + page.
                    if target != *page as i32 {
                        self.lectern_button(key, 100 + target);
                    }
                } else if target != *page as i32 {
                    *page = target as usize;
                }
            }
            ClickEvent::OpenUrl(url) => self.click_url(url),
            // closeContainerOnServer, then clickCommandAction with no screen
            // after the command: the book closes and the command runs. A
            // lectern's screen goes with its menu; a book is still the screen
            // a confirmation returns to.
            ClickEvent::RunCommand(command) => {
                let book = if lectern.is_none() {
                    self.book.take()
                } else {
                    None
                };
                self.close_book();
                self.click_command(&command);
                if let Some(book) = book {
                    self.command_prompt_returns_to(super::remote_chat::Behind::Book(Box::new(
                        book,
                    )));
                }
            }
            // Screen.defaultHandleClickEvent: copied, the book stays open.
            ClickEvent::CopyToClipboard(value) => {
                use horizon_client::clipboard::Clipboard;
                self.clipboard.set(&value);
            }
            // defaultHandleGameClickEvent: the dialog opens over the book.
            ClickEvent::ShowDialog(dialog) => self.show_dialog_ref(&dialog),
            // Screen.defaultHandleGameClickEvent: custom actions retain the book.
            ClickEvent::Custom { id, payload } => {
                if let Some(remote) = &self.remote {
                    remote.session.send_custom_click(&id, payload);
                }
            }
            // SuggestCommand's insertText does nothing on a book; a server's
            // text never opens a file.
            ClickEvent::SuggestCommand(_) | ClickEvent::OpenFile(_) => {}
        }
    }
    /// `WritableBookItem.use` / `WrittenBookItem.use` (`Player.openItemGui`).
    /// A server's player writes in its own client's editor
    /// (`LocalPlayer.openItemGui`), and reads on its own client once told to
    /// (`ServerPlayer.openItemGui`'s ClientboundOpenBookPacket).
    pub(super) fn use_book(&mut self, stack: &ItemStack) -> bool {
        let written = match &*stack.item {
            "minecraft:writable_book" => false,
            "minecraft:written_book" => true,
            _ => return false,
        };
        // `ServerPlayer.openItemGui` resolves a written book for the player
        // opening it before the screen reads it.
        let held;
        let stack = if written && self.resolve_held_book() {
            held = self.held().cloned();
            held.as_ref().unwrap_or(stack)
        } else {
            stack
        };
        if self.lan_acting.is_some() {
            if written {
                let hand = i32::from(self.hand == Hand::Off);
                self.guest_packets
                    .push(C::OpenBook(play::OpenBook { hand }));
            }
        } else if let Some(screen) = self.book_screen(stack) {
            self.open_book(screen);
        }
        // WritableBookItem.use / WrittenBookItem.use count every open.
        self.award_stat("used", &stack.item, 1);
        self.swing_arm();
        true
    }
    /// `ServerPlayer.openItemGui` -> `WrittenBookContent.resolveForItem`: the
    /// selectors and scores of the written book in the acting hand become
    /// their text for the player opening it, and the book is marked resolved
    /// (so it is never resolved again). True when the pages were rewritten.
    fn resolve_held_book(&mut self) -> bool {
        let Some(mut book) = self
            .held()
            .and_then(|s| s.data())
            .and_then(|d| d.written.clone())
        else {
            return false;
        };
        if book.resolved {
            return false;
        }
        let string = |v: &Tag| match v {
            Tag::String(s) => Some(s.to_string_lossy()),
            _ => None,
        };
        let changed = book.resolve(|tag| {
            let Tag::Compound(c) = tag else { return None };
            if let Some(Tag::String(selector)) = c.get(&"selector".into()) {
                let selector = selector.to_string_lossy();
                return Some(self.resolve_selector_text(&selector));
            }
            let score = c.get(&"score".into())?;
            let name = score.get("name").and_then(string).unwrap_or_default();
            let objective = score.get("objective").and_then(string).unwrap_or_default();
            let name = if name == "*" {
                PLAYER_NAME
            } else {
                name.as_str()
            };
            Some(
                self.score
                    .board
                    .score(name, &objective)
                    .map_or_else(String::new, |s| s.value.to_string()),
            )
        });
        if let Some(stack) = self.held_mut() {
            stack.data_mut().written = Some(book);
        }
        // A guest's copy of the book lives in its mirrored inventory: the
        // host sends the resolved slot back with the rest of the inventory.
        if self.lan_acting.is_some() {
            self.guest_inventory_resend = true;
        }
        changed
    }
    /// The screen a book in hand opens: `BookEditScreen` for a book and
    /// quill, `BookViewScreen` for a written book.
    fn book_screen(&self, stack: &ItemStack) -> Option<BookScreen> {
        Some(match &*stack.item {
            "minecraft:writable_book" => {
                let mut pages = stack
                    .data()
                    .map(|d| d.writable_pages.clone())
                    .unwrap_or_default();
                if pages.is_empty() {
                    pages.push(String::new());
                }
                let mut page_edit = book_page_box();
                page_edit.set_value(&pages[0]);
                BookScreen::Edit {
                    slot: self.held_slot(),
                    pages,
                    page: 0,
                    signing: false,
                    title: String::new(),
                    title_edit: book_title_box(),
                    page_edit,
                }
            }
            "minecraft:written_book" => BookScreen::View {
                pages: view_pages(stack.data()),
                page: 0,
                lectern: None,
            },
            _ => return None,
        })
    }
    /// `MultiPlayerGameMode.useItem`'s own use of a book and quill on a
    /// server's client: its editor opens here at once.
    pub(super) fn remote_use_book(&mut self, stack: &ItemStack) -> bool {
        if &*stack.item != "minecraft:writable_book" {
            return false;
        }
        if let Some(screen) = self.book_screen(stack) {
            self.open_book(screen);
        }
        true
    }
    /// `ClientPacketListener.handleOpenBook`: the written book in that hand.
    pub(super) fn remote_open_book(&mut self, hand: i32) {
        let hand = if hand == 1 { Hand::Off } else { Hand::Main };
        let screen = self.with_hand(hand, |g| {
            let stack = g.held().filter(|s| &*s.item == "minecraft:written_book")?;
            g.book_screen(&stack.clone())
        });
        if let Some(screen) = screen {
            self.open_book(screen);
        }
    }
    fn open_book(&mut self, screen: BookScreen) {
        self.book = Some(screen);
        self.keys.clear();
        self.right = false;
        self.capture(false);
    }
    pub(super) fn close_book(&mut self) {
        // A server's lectern: LecternScreen's menu closes (closeContainer).
        if matches!(
            self.book,
            Some(BookScreen::View {
                lectern: Some(_),
                ..
            })
        ) && self.remote.is_some()
        {
            self.remote_close_container();
        }
        self.book = None;
        self.capture(self.screen == super::Screen::Playing && !self.screen_over_playing());
    }
    /// `BookEditScreen.saveChanges` and `BookSignScreen`'s Sign and Close:
    /// trailing empty pages dropped, the book in hand updated here and, on a
    /// server's client, sent (ServerboundEditBookPacket).
    fn save_book(&mut self, sign: bool) {
        let Some(BookScreen::Edit {
            slot, pages, title, ..
        }) = self.book.take()
        else {
            return;
        };
        let mut pages = pages;
        while pages.len() > 1 && pages.last().is_some_and(String::is_empty) {
            pages.pop();
        }
        let title = sign.then(|| title.trim().to_owned());
        if self.remote.is_some() {
            self.send(S::EditBook(play::EditBook {
                slot: slot as i32,
                pages: pages.clone(),
                title: title.clone(),
            }));
        }
        self.write_book(slot, pages, title);
        self.capture(true);
    }
    /// The book and quill at `slot` takes the pages, or with a title becomes
    /// the written book its writer signed.
    fn write_book(&mut self, slot: usize, pages: Vec<String>, title: Option<String>) {
        let author = self
            .lan_acting_name
            .clone()
            .unwrap_or_else(|| super::chat::PLAYER_NAME.to_owned());
        let Some(stack) = self
            .stack_in_mut(slot)
            .as_mut()
            .filter(|s| &*s.item == "minecraft:writable_book")
        else {
            return;
        };
        if let Some(title) = title {
            let mut written = ItemStack::new("minecraft:written_book", 1);
            written.data_mut().written = Some(WrittenBook {
                title,
                author,
                generation: 0,
                pages,
                page_tags: Vec::new(),
                resolved: false,
            });
            *stack = written;
        } else {
            stack.data_mut().writable_pages = if pages.iter().all(String::is_empty) {
                Vec::new()
            } else {
                pages
            };
            stack.tidy();
        }
    }
    /// `ServerGamePacketListenerImpl.handleEditBook`: a guest's book in a
    /// hotbar slot or its off hand (`updateBookContents`, `signBook`).
    pub(super) fn lan_edit_book(&mut self, host: &mut LanHost, conn: u32, p: play::EditBook) {
        if !((0..9).contains(&p.slot) || p.slot == super::shield::OFFHAND_SLOT as i32) {
            return;
        }
        if let Some(title) = &p.title
            && !self.chat_filter_text(
                host,
                conn,
                &[title],
                horizon_client::lan::hub::chat_filter::Place::Book,
            )
        {
            return;
        }
        let menu_slot = if p.slot == super::shield::OFFHAND_SLOT as i32 {
            45
        } else {
            36 + p.slot as usize
        };
        let cancelled = self.as_guest(host, conn, |g| {
            let mut cancel = false;
            let writable = g
                .stack_in(p.slot as usize)
                .is_some_and(|stack| &*stack.item == "minecraft:writable_book");
            if writable
                && let (Some(player), Some(name)) = (g.lan_acting_uuid, g.lan_acting_name.clone())
            {
                g.native_player_edit_book(
                    player,
                    &name,
                    p.pages.len() as i32,
                    p.title.is_some(),
                    &mut cancel,
                );
            }
            if cancel {
                return true;
            }
            g.write_book(p.slot as usize, p.pages, p.title);
            false
        });
        // A cancelled edit leaves the client's own copy of the book: forget
        // what was sent so the next broadcast resends the server's stack.
        if cancelled == Some(true)
            && let Some(g) = host.hub.guests.iter_mut().find(|g| g.conn == conn)
        {
            g.menu_sent[menu_slot] = None;
        }
    }
    pub(super) fn book_key(&mut self, event: &KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        if let Some(shortcut) = self.edit_shortcut(event) {
            self.book_shortcut(shortcut);
            return;
        }
        let key = match event.physical_key {
            PhysicalKey::Code(k) => Some(k),
            _ => None,
        };
        self.book_input(key, event.text.as_deref());
    }
    /// A key and the text it typed in the open book (the keyboard, or a
    /// controller's on-screen keyboard).
    pub(super) fn book_input(&mut self, key: Option<KeyCode>, text: Option<&str>) {
        match self.book.as_mut() {
            Some(BookScreen::View { .. }) => match key {
                Some(KeyCode::Escape) => self.close_book(),
                Some(KeyCode::PageUp) => self.turn_page(false),
                Some(KeyCode::PageDown) => self.turn_page(true),
                _ => {}
            },
            Some(BookScreen::Edit { signing: true, .. }) => match key {
                Some(KeyCode::Escape) => {
                    if let Some(BookScreen::Edit { signing, .. }) = self.book.as_mut() {
                        *signing = false;
                    }
                }
                Some(KeyCode::Enter | KeyCode::NumpadEnter) => {
                    let blank = self.book.as_ref().is_some_and(
                        |b| matches!(b, BookScreen::Edit { title, .. } if title.trim().is_empty()),
                    );
                    if !blank {
                        self.save_book(true);
                    }
                }
                Some(key) if self.book_title_key(key) => {}
                _ => {
                    if let Some(typed) = text {
                        self.book_title_insert(typed);
                    }
                }
            },
            Some(BookScreen::Edit { .. }) => match key {
                Some(KeyCode::Escape) => self.save_book(false),
                Some(KeyCode::PageUp) => self.turn_page(false),
                Some(KeyCode::PageDown) => self.turn_page(true),
                Some(KeyCode::ArrowUp) => self.book_move_line(-1),
                Some(KeyCode::ArrowDown) => self.book_move_line(1),
                // MultilineTextField.keyPressed: Home and End seek the caret's
                // line; with Ctrl they go to the page's start and end.
                Some(edge @ (KeyCode::Home | KeyCode::End)) if !self.has_control_down() => {
                    self.book_line_edge(edge == KeyCode::End)
                }
                Some(KeyCode::Enter | KeyCode::NumpadEnter) => self.page_insert("\n"),
                Some(key) if self.book_page_key(key) => {}
                _ => {
                    if let Some(typed) = text {
                        self.page_insert(typed);
                    }
                }
            },
            None => {}
        }
    }
    /// Typing for `horizon-ctl type` (`\n` is a line break).
    pub(super) fn book_type(&mut self, text: &str) {
        let text = text.replace("\\n", "\n");
        match self.book.as_mut() {
            Some(BookScreen::Edit { signing: true, .. }) => self.book_title_insert(&text),
            Some(BookScreen::Edit { .. }) => self.page_insert(&text),
            _ => {}
        }
    }
    /// `BookSignScreen.keyPressed` on the title box.
    fn book_title_key(&mut self, key: KeyCode) -> bool {
        let (ctrl, shift) = (self.has_control_down(), self.has_shift_down());
        let Some(BookScreen::Edit {
            title, title_edit, ..
        }) = self.book.as_mut()
        else {
            return false;
        };
        if !title_edit.key(key, ctrl, shift) {
            return false;
        }
        title.clone_from(&title_edit.value);
        true
    }
    fn book_title_insert(&mut self, text: &str) {
        let Some(BookScreen::Edit {
            title, title_edit, ..
        }) = self.book.as_mut()
        else {
            return;
        };
        title_edit.insert_text(text);
        title.clone_from(&title_edit.value);
    }
    /// `BookEditScreen.keyPressed` on the page, apart from the line moves.
    fn book_page_key(&mut self, key: KeyCode) -> bool {
        let (ctrl, shift) = (self.has_control_down(), self.has_shift_down());
        let before = match self.book.as_ref() {
            Some(BookScreen::Edit { page_edit, .. }) => Some((
                page_edit.value.clone(),
                page_edit.cursor,
                page_edit.highlight,
            )),
            _ => return false,
        };
        let handled = if let Some(BookScreen::Edit { page_edit, .. }) = self.book.as_mut() {
            page_edit.key(key, ctrl, shift)
        } else {
            false
        };
        if handled {
            self.page_keep(before);
        }
        handled
    }
    /// One character at a time, so a page stops at the last character that fits.
    fn page_insert(&mut self, text: &str) {
        for c in text.chars() {
            self.page_change(|edit| edit.insert_text(&c.to_string()));
        }
    }
    /// Ctrl+A/C/V/X: the title box while signing, else the page.
    pub(super) fn book_shortcut(&mut self, shortcut: horizon_client::clipboard::EditShortcut) {
        let signing = matches!(self.book, Some(BookScreen::Edit { signing: true, .. }));
        if signing {
            if let Some(BookScreen::Edit {
                title, title_edit, ..
            }) = self.book.as_mut()
            {
                title_edit.shortcut(shortcut, &mut self.clipboard);
                title.clone_from(&title_edit.value);
            }
            return;
        }
        let before = match self.book.as_ref() {
            Some(BookScreen::Edit { page_edit, .. }) => Some((
                page_edit.value.clone(),
                page_edit.cursor,
                page_edit.highlight,
            )),
            _ => None,
        };
        if let Some(BookScreen::Edit { page_edit, .. }) = self.book.as_mut() {
            page_edit.shortcut(shortcut, &mut self.clipboard);
        }
        self.page_keep(before);
    }
    /// Edit the page box, then keep it only within 14 lines and 1024 characters.
    fn page_change(&mut self, f: impl FnOnce(&mut ui::EditBox)) -> bool {
        let before = match self.book.as_ref() {
            Some(BookScreen::Edit { page_edit, .. }) => Some((
                page_edit.value.clone(),
                page_edit.cursor,
                page_edit.highlight,
            )),
            _ => return false,
        };
        let ran = if let Some(BookScreen::Edit { page_edit, .. }) = self.book.as_mut() {
            f(page_edit);
            true
        } else {
            false
        };
        ran && self.page_keep(before)
    }
    /// Write the page, or put the previous text back when it no longer fits.
    fn page_keep(&mut self, before: Option<(String, usize, usize)>) -> bool {
        let next = match self.book.as_ref() {
            Some(BookScreen::Edit { page_edit, .. }) => page_edit.value.clone(),
            _ => return false,
        };
        let ok = next.chars().count() <= PAGE_CHARS && self.book_lines(&next).len() <= LINES;
        let Some(BookScreen::Edit {
            pages,
            page,
            page_edit,
            ..
        }) = self.book.as_mut()
        else {
            return false;
        };
        if ok {
            pages[*page].clone_from(&page_edit.value);
        } else if let Some((value, cursor, highlight)) = before {
            page_edit.value = value;
            page_edit.cursor = cursor;
            page_edit.highlight = highlight;
        }
        ok
    }
    /// ArrowUp and ArrowDown move a visual line, keeping the column.
    fn book_move_line(&mut self, dir: isize) {
        let shift = self.has_shift_down();
        let (text, cursor) = match self.book.as_ref() {
            Some(BookScreen::Edit {
                pages,
                page,
                page_edit,
                ..
            }) => (pages[*page].clone(), page_edit.cursor),
            _ => return,
        };
        let rows = self.page_rows(&text);
        let (x, line) = self.caret_on(&text, &rows, cursor);
        let next = (line as isize + dir).clamp(0, rows.len() as isize - 1) as usize;
        let (start, end) = rows[next];
        let chars: Vec<char> = text.chars().collect();
        let slice: String = chars[start..end].iter().collect();
        // MultilineTextField.LINE_SEEK_PIXEL_BIAS: vanilla biases this seek, not the drawn caret.
        let pos = start + self.gui.atlas.font.fit(&slice, x + 2.0);
        if let Some(BookScreen::Edit { page_edit, .. }) = self.book.as_mut() {
            page_edit.set_cursor(pos, shift);
        }
    }
    /// Home and End: the start or end of the visual line holding the caret
    /// (`getCursorLineView`'s beginIndex and endIndex).
    fn book_line_edge(&mut self, end: bool) {
        let shift = self.has_shift_down();
        let (text, cursor) = match self.book.as_ref() {
            Some(BookScreen::Edit {
                pages,
                page,
                page_edit,
                ..
            }) => (pages[*page].clone(), page_edit.cursor),
            _ => return,
        };
        let rows = self.page_rows(&text);
        let (_, line) = self.caret_on(&text, &rows, cursor);
        let (start, stop) = rows[line];
        if let Some(BookScreen::Edit { page_edit, .. }) = self.book.as_mut() {
            page_edit.set_cursor(if end { stop } else { start }, shift);
        }
    }
    /// Wrapped rows of a page as character ranges, the same breaks as `font.split`.
    fn page_rows(&self, page: &str) -> Vec<(usize, usize)> {
        let font = &self.gui.atlas.font;
        let chars: Vec<char> = page.chars().collect();
        let mut rows = Vec::new();
        let mut start = 0;
        while start < chars.len() {
            let mut end = chars.len();
            let mut next = chars.len();
            let mut last_space = None;
            for i in start..chars.len() {
                if chars[i] == '\n' {
                    end = i;
                    next = i + 1;
                    break;
                }
                if chars[i] == ' ' {
                    last_space = Some(i);
                }
                let slice: String = chars[start..=i].iter().collect();
                if font.width(&slice) > 114.0 {
                    match last_space {
                        Some(s) if s > start => {
                            end = s;
                            next = s + 1;
                        }
                        _ => {
                            end = i.max(start + 1);
                            next = end;
                        }
                    }
                    break;
                }
            }
            rows.push((start, end));
            start = next;
            if start == chars.len() && chars.last() == Some(&'\n') {
                rows.push((chars.len(), chars.len()));
            }
        }
        if rows.is_empty() {
            rows.push((0, 0));
        }
        rows
    }
    /// GUI x of the caret and the row it sits on.
    fn caret_on(&self, page: &str, rows: &[(usize, usize)], cursor: usize) -> (f32, usize) {
        let chars: Vec<char> = page.chars().collect();
        for (i, &(start, end)) in rows.iter().enumerate() {
            if cursor <= end || i + 1 == rows.len() {
                let take = cursor.saturating_sub(start).min(end.saturating_sub(start));
                let prefix: String = chars.iter().skip(start).take(take).collect();
                return (self.gui.atlas.font.width(&prefix), i);
            }
        }
        (0.0, 0)
    }
    fn page_index_at(&self, page: &str, local_x: f32, local_y: f32) -> usize {
        let rows = self.page_rows(page);
        let line = (local_y / 9.0).floor().max(0.0) as usize;
        let line = line.min(rows.len().saturating_sub(1));
        let (start, end) = rows[line];
        let chars: Vec<char> = page.chars().collect();
        let slice: String = chars[start..end].iter().collect();
        start + self.gui.atlas.font.fit(&slice, local_x.max(0.0))
    }
    /// A click on the centred title (`EditBox.onClick`).
    fn book_title_click(&mut self, field_x: f32, mx: f32) {
        let shift = self.has_shift_down();
        let Some(BookScreen::Edit {
            title, title_edit, ..
        }) = self.book.as_mut()
        else {
            return;
        };
        let tx = field_x + ((114.0 - self.gui.atlas.font.width(title)) / 2.0).floor();
        let rel = (mx - tx).max(0.0);
        let pos = self.gui.atlas.font.fit(title, rel);
        title_edit.set_cursor(pos, shift);
    }
    /// A click on the open page (`MultiLineEditBox.onClick`): a double click
    /// selects the word before the cursor and leaves the cursor where it is.
    fn book_page_click(&mut self, origin_x: f32, origin_y: f32, mx: f32, my: f32, double: bool) {
        if double {
            if let Some(BookScreen::Edit { page_edit, .. }) = self.book.as_mut() {
                page_edit.select_word_at_cursor();
            }
            return;
        }
        let shift = self.has_shift_down();
        let text = match self.book.as_ref() {
            Some(BookScreen::Edit { pages, page, .. }) => pages[*page].clone(),
            _ => return,
        };
        let pos = self.page_index_at(&text, mx - origin_x, my - origin_y);
        if let Some(BookScreen::Edit { page_edit, .. }) = self.book.as_mut() {
            page_edit.set_cursor(pos, shift);
        }
    }
    /// The page buttons (`pageForward` adds a page while editing).
    fn turn_page(&mut self, forward: bool) {
        let mut lectern_page = None;
        match self.book.as_mut() {
            Some(BookScreen::Edit {
                pages,
                page,
                page_edit,
                ..
            }) => {
                let before = *page;
                if forward {
                    if *page + 1 >= pages.len() && pages.len() < MAX_PAGES {
                        pages.push(String::new());
                    }
                    if *page + 1 < pages.len() {
                        *page += 1;
                    }
                } else if *page > 0 {
                    *page -= 1;
                }
                if *page != before {
                    let text = pages[*page].clone();
                    page_edit.set_value(&text);
                }
            }
            Some(BookScreen::View {
                lectern: Some(key), ..
            }) => lectern_page = Some(*key),
            Some(BookScreen::View { pages, page, .. }) => {
                if forward && *page + 1 < pages.len() {
                    *page += 1;
                } else if !forward && *page > 0 {
                    *page -= 1;
                }
            }
            None => return,
        }
        // LecternScreen's page buttons click the menu's (1 back, 2 forward);
        // the page turns with the server's data slot and level event 1043.
        if let Some(key) = lectern_page {
            self.lectern_button(key, if forward { 2 } else { 1 });
            return;
        }
        self.audio
            .play("minecraft:item.book.page_turn", None, 1.0, 1.0);
    }
    fn set_lectern_powered(&mut self, key: (i32, i32, i32), powered: bool) {
        self.set_lectern_property(key, "powered", powered);
        let p = BlockPos::new(key.0, key.1, key.2);
        let state = self.world.block(key.0, key.1, key.2);
        self.with_level(|b, level, ticks| {
            b.lectern_update_below(level, ticks, p, state);
        });
    }
    /// `LecternBlock.resetBookState`: the property with the block change event.
    fn set_lectern_has_book(&mut self, key: (i32, i32, i32), has_book: bool, who: Option<Who>) {
        // `LecternBlock.resetBookState`: `setBlockAndUpdate` writes the state and
        // the `block_change` post follows unconditionally, not only when the
        // state differs.
        self.set_lectern_property(key, "has_book", has_book);
        let after = self.world.block(key.0, key.1, key.2);
        self.post_block_game_event(
            "minecraft:block_change",
            [key.0, key.1, key.2],
            who,
            Some(after),
        );
    }
    fn set_lectern_property(&mut self, key: (i32, i32, i32), name: &str, value: bool) {
        let state = self.world.block(key.0, key.1, key.2);
        let registry = self.pack.assets.registry;
        let Some(mut s) = registry.state(state).cloned() else {
            return;
        };
        if !s.properties.contains_key(name) {
            return;
        }
        s.properties.insert(name.into(), value.to_string());
        if let Some(new) = registry.id(&s) {
            let p = BlockPos::new(key.0, key.1, key.2);
            self.with_level(|b, level, ticks| {
                b.set_block(level, ticks, p, new, level_flags::UPDATE_ALL)
            });
        }
    }
    /// The 2-tick pulses of turned lecterns.
    pub(super) fn tick_lecterns(&mut self) {
        self.follow_lectern();
        let mut done = Vec::new();
        for (key, left) in &mut self.lectern_pulses {
            *left -= 1;
            if *left <= 0 {
                done.push(*key);
            }
        }
        self.lectern_pulses.retain(|(_, left)| *left > 0);
        for key in done {
            self.set_lectern_powered(key, false);
        }
    }
    /// `LecternBlock.getStateForPlacement`: a lectern placed from an item
    /// whose `block_entity_data` holds a `Book`, by a placer who may use
    /// game-master blocks, starts with `has_book` on. It runs before the
    /// item's `block_state` component, which vanilla applies afterwards and
    /// which may set `has_book` back. Returns the state to go on with.
    pub(super) fn lectern_placed(
        &mut self,
        pos: [i32; 3],
        state: BlockStateId,
        stack: &ItemStack,
        game_master: bool,
    ) -> horizon_client::Result<BlockStateId> {
        if !game_master
            || !block_entity_item::carries_book(stack)
            || &*self.res.facts().get(state).name != "minecraft:lectern"
        {
            return Ok(state);
        }
        match self.with_props(state, &[("has_book", "true")]) {
            Some(new) if new != state => {
                self.level_set(pos, new)?;
                Ok(new)
            }
            _ => Ok(state),
        }
    }
    /// `LecternBlock.useItemOn`: a book goes onto an empty lectern.
    pub(super) fn use_lectern(&mut self, pos: [i32; 3], held: Option<&ItemStack>) -> bool {
        let key = (pos[0], pos[1], pos[2]);
        if &*self
            .res
            .facts()
            .get(self.world.block(pos[0], pos[1], pos[2]))
            .name
            != "minecraft:lectern"
        {
            return false;
        }
        let has_book = matches!(self.world.block_entities.get(&key), Some(BlockEntity::Lectern(l)) if l.book.is_some());
        // `LecternBlock.useItemOn`: `ItemTags.LECTERN_BOOKS`, which a data
        // pack can add its own books to, not only the two vanilla items.
        let Some(book) = held.filter(|h| {
            world_items!(self)
                .item_tags
                .get("minecraft:lectern_books")
                .is_some_and(|books| books.contains(&*h.item))
        }) else {
            return false;
        };
        if has_book {
            return false;
        }
        let mut one = book.clone();
        one.count = 1;
        self.world.block_entities.insert(
            key,
            BlockEntity::Lectern(Lectern {
                book: Some(one),
                page: 0,
            }),
        );
        self.set_lectern_has_book(key, true, self.remote.is_none().then_some(Who::Player));
        if !self.creative {
            self.consume_held();
        }
        let at = [
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]) + 0.5,
            f64::from(pos[2]) + 0.5,
        ];
        self.audio
            .play("minecraft:item.book.put", Some(at), 1.0, 1.0);
        self.container_changed(key);
        if let (Some(player), Some(name)) = (self.lan_acting_uuid, self.lan_acting_name.clone()) {
            self.native_player_insert_lectern_book(player, &name, pos, &book.item);
        }
        true
    }
    /// `LecternBlock.openScreen`: reading the lectern's book.
    pub(super) fn open_lectern(&mut self, pos: [i32; 3]) -> bool {
        let key = (pos[0], pos[1], pos[2]);
        let Some(BlockEntity::Lectern(l)) = self.world.block_entities.get(&key) else {
            return false;
        };
        let Some(book) = &l.book else { return false };
        let pages = view_pages(book.data());
        let page = (l.page.max(0) as usize).min(pages.len().saturating_sub(1));
        self.award_custom_stat("interact_with_lectern", 1);
        self.open_book(BookScreen::View {
            pages,
            page,
            lectern: Some(key),
        });
        true
    }
    /// A `LecternMenu` button of the open lectern screen: on a server it is
    /// sent (`handleInventoryButtonClick`); here the lectern's menu takes it
    /// with the player's inventory, and the screen follows the block entity.
    fn lectern_button(&mut self, key: (i32, i32, i32), id: i32) {
        if self.remote.is_some() {
            if let Some((container, _)) = self.remote.as_ref().and_then(|r| r.menu.as_ref()) {
                self.send(
                    horizon_protocol::packets::play::Serverbound::ContainerButtonClick(
                        horizon_protocol::packets::play::ContainerButtonClick {
                            container: *container,
                            button: id,
                        },
                    ),
                );
            }
            return;
        }
        let Some(mut menu) = self.lectern_menu(key) else {
            return;
        };
        for i in 0..36 {
            menu.set_player_item(i, self.inventory[i].clone());
        }
        menu.set_player_item(40, self.offhand.clone());
        menu.creative = self.creative;
        menu.selected = self.selected;
        menu.may_build = self.may_build();
        if !menu.button(&self.menu_env(), id) {
            return;
        }
        for i in 0..36 {
            self.inventory[i] = menu.player_item(i).cloned();
        }
        self.offhand = menu.player_item(40).cloned();
        for stack in std::mem::take(&mut menu.dropped) {
            self.player_drop(stack);
        }
        self.lectern_menu_done(key, &menu);
        self.follow_lectern();
    }
    /// LecternScreen's Take Book: the menu's button 3. The book leaves the
    /// lectern, so the screen closes (the server closes the menu once the
    /// book is gone; a server's client closes at once rather than wait).
    fn take_lectern_book(&mut self, key: (i32, i32, i32)) {
        let before = match self.world.block_entities.get(&key) {
            Some(BlockEntity::Lectern(l)) => l.book.clone(),
            _ => None,
        };
        self.lectern_button(key, 3);
        if let (Some(book), Some(player), Some(name)) = (
            before.filter(|_| {
                matches!(self.world.block_entities.get(&key),
                Some(BlockEntity::Lectern(l)) if l.book.is_none())
            }),
            self.lan_acting_uuid,
            self.lan_acting_name.clone(),
        ) {
            self.native_player_take_lectern_book(player, &name, [key.0, key.1, key.2], &book.item);
        }
        if self.may_build()
            && matches!(
                self.book,
                Some(BookScreen::View {
                    lectern: Some(_),
                    ..
                })
            )
        {
            self.close_book();
        }
    }
    /// The `book` control command: a button of the open book or lectern
    /// screen (the page arrows, Take Book, Done), then its page.
    pub(super) fn book_control(
        &mut self,
        action: Option<&str>,
    ) -> horizon_client::Result<serde_json::Value> {
        let lectern = match &self.book {
            Some(BookScreen::View { lectern, .. }) => *lectern,
            Some(BookScreen::Edit { .. }) => None,
            None => return Err("no book is open".into()),
        };
        match action {
            None => {}
            Some("next") => self.turn_page(true),
            Some("back") => self.turn_page(false),
            Some("take") => match lectern {
                Some(key) => self.take_lectern_book(key),
                None => return Err("not a lectern".into()),
            },
            // The book and quill's buttons: Sign, then Sign and Close; Done
            // (which saves), then Cancel.
            Some("sign") => match self.book.as_mut() {
                Some(BookScreen::Edit {
                    signing: true,
                    title,
                    ..
                }) if !title.trim().is_empty() => self.save_book(true),
                Some(book @ BookScreen::Edit { .. }) => book.begin_signing(),
                _ => return Err("not a book and quill".into()),
            },
            Some("done") => match self.book.as_mut() {
                Some(BookScreen::Edit { signing: true, .. }) => {
                    if let Some(BookScreen::Edit { signing, .. }) = self.book.as_mut() {
                        *signing = false;
                    }
                }
                Some(BookScreen::Edit { .. }) => self.save_book(false),
                _ => self.close_book(),
            },
            Some(other) => return Err(format!("unknown book button {other}").into()),
        }
        Ok(match &self.book {
            Some(BookScreen::View {
                pages,
                page,
                lectern,
            }) => serde_json::json!({
                "page": page, "pages": pages.len(), "lectern": lectern.map(|k| [k.0, k.1, k.2]),
            }),
            Some(BookScreen::Edit { pages, page, .. }) => {
                serde_json::json!({ "page": page, "pages": pages.len(), "lectern": null })
            }
            None => serde_json::json!({ "closed": true }),
        })
    }
    /// A server's `minecraft:lectern` menu: LecternScreen, its book and
    /// page to come (`remote_lectern_book`, `remote_lectern_page`).
    pub(super) fn remote_open_lectern(&mut self, key: (i32, i32, i32)) {
        self.open_book(BookScreen::View {
            pages: Vec::new(),
            page: 0,
            lectern: Some(key),
        });
    }
    /// `LecternScreen.bookChanged`: the menu's book slot.
    pub(super) fn remote_lectern_book(&mut self, book: Option<ItemStack>) {
        if let Some(BookScreen::View {
            lectern: Some(_),
            pages,
            page,
        }) = self.book.as_mut()
        {
            *pages = view_pages(book.as_ref().and_then(ItemStack::data));
            *page = (*page).min(pages.len().saturating_sub(1));
        }
    }
    /// `LecternScreen.pageChanged`: the menu's page data slot.
    pub(super) fn remote_lectern_page(&mut self, value: i32) {
        if let Some(BookScreen::View {
            lectern: Some(_),
            pages,
            page,
        }) = self.book.as_mut()
        {
            *page = (value.max(0) as usize).min(pages.len().saturating_sub(1));
        }
    }
    /// The server closed its lectern menu; false when none is open.
    pub(super) fn remote_lectern_closed(&mut self) -> bool {
        if !matches!(
            self.book,
            Some(BookScreen::View {
                lectern: Some(_),
                ..
            })
        ) {
            return false;
        }
        if let Some(r) = self.remote.as_mut() {
            r.menu = None;
            r.menu_title = None;
        }
        self.book = None;
        self.capture(true);
        true
    }
    /// `LecternMenu` over the lectern at `key` (the menu engine, for the
    /// local player and LAN guests alike): its book and page.
    pub(super) fn lectern_menu(&self, key: (i32, i32, i32)) -> Option<Menu> {
        let Some(BlockEntity::Lectern(l)) = self.world.block_entities.get(&key) else {
            return None;
        };
        let mut menu = Menu::new(Kind::Lectern);
        menu.set_slot(&self.menu_env(), 0, l.book.clone());
        menu.data[0] = l.page;
        Some(menu)
    }
    /// The lectern menu's book and page back in the block entity: a new page
    /// is `LecternBlock.signalPageChange` (the 2-tick pulse, level event
    /// 1043), a book gone `onBookItemRemove` (`resetBookState`).
    pub(super) fn lectern_menu_done(&mut self, key: (i32, i32, i32), menu: &Menu) {
        let Some(BlockEntity::Lectern(l)) = self.world.block_entities.get_mut(&key) else {
            return;
        };
        let (had, before) = (l.book.is_some(), l.page);
        l.book = menu.slot(0).cloned();
        l.page = menu.data[0];
        if had && l.book.is_none() {
            self.set_lectern_powered(key, false);
            self.set_lectern_has_book(key, false, None);
            self.container_changed(key);
        } else if menu.data[0] != before {
            self.set_lectern_powered(key, true);
            self.lectern_pulses.push((key, 2));
            self.level_event_at(1043, [key.0, key.1, key.2], 0);
            self.container_changed(key);
            if let (Some(player), Some(name)) = (self.lan_acting_uuid, self.lan_acting_name.clone())
            {
                self.native_player_lectern_page_change(
                    player,
                    &name,
                    [key.0, key.1, key.2],
                    menu.data[0],
                );
            }
        }
    }
    /// The open lectern screen follows its block entity (`LecternScreen`'s
    /// menu listener): the page, and closing once the book is gone or the
    /// player is out of `LecternMenu.stillValid`'s reach.
    pub(super) fn follow_lectern(&mut self) {
        let Some(BookScreen::View {
            lectern: Some(key), ..
        }) = self.book.as_ref()
        else {
            return;
        };
        if self.remote.is_some() {
            return;
        }
        let key = *key;
        // `LecternMenu.stillValid` -> `LecternBlockEntity$1.stillValid`:
        // `Container.stillValidBlockEntity` (the same block entity) and
        // `hasBook()`, plus the player's reach with the 4-block buffer.
        let has_book = matches!(
            self.world.block_entities.get(&key),
            Some(BlockEntity::Lectern(l)) if l.book.is_some()
        );
        let in_reach = self.eye_position().is_some_and(|eye| {
            horizon_client::lan::guest::within_block_range(
                eye,
                self.reach() + 4.0,
                [key.0, key.1, key.2],
            )
        });
        if !has_book || !in_reach {
            self.close_book();
            return;
        }
        let Some(BookScreen::View { page, pages, .. }) = self.book.as_mut() else {
            return;
        };
        if let Some(BlockEntity::Lectern(l)) = self.world.block_entities.get(&key) {
            *page = (l.page.max(0) as usize).min(pages.len().saturating_sub(1));
        }
    }
    pub(super) fn book_click(&mut self, size: [u32; 2]) {
        let b = HudBatch::new(size);
        let (w, left) = (b.width, ((b.width - 192.0) / 2.0).floor());
        let (mx, my) = self.cursor;
        // MouseHandler: a click within 250 ms of the last one on this screen is a double click.
        let now = std::time::Instant::now();
        let double = self
            .book_last_click
            .is_some_and(|at| now.duration_since(at).as_millis() < 250);
        self.book_last_click = Some(now);
        let inside =
            |x: f32, y: f32, bw: f32, bh: f32| mx >= x && my >= y && mx < x + bw && my < y + bh;
        let click = |game: &mut Self| {
            game.audio
                .play("minecraft:ui.button.click", None, 0.25, 1.0)
        };
        match &self.book {
            Some(BookScreen::Edit {
                signing: true,
                title,
                ..
            }) => {
                let blank = title.trim().is_empty();
                if inside(w / 2.0 - 100.0, 196.0, 98.0, 20.0) && !blank {
                    click(self);
                    self.save_book(true);
                } else if inside(w / 2.0 + 2.0, 196.0, 98.0, 20.0) {
                    click(self);
                    if let Some(BookScreen::Edit { signing, .. }) = self.book.as_mut() {
                        *signing = false;
                    }
                } else if inside(left + 36.0, 50.0, 114.0, 9.0) {
                    self.book_title_click(left + 36.0, mx);
                }
            }
            Some(BookScreen::Edit { page, .. }) => {
                let page = *page;
                if inside(left + 116.0, 159.0, 23.0, 13.0) {
                    self.turn_page(true);
                } else if page > 0 && inside(left + 43.0, 159.0, 23.0, 13.0) {
                    self.turn_page(false);
                } else if inside(w / 2.0 - 100.0, 196.0, 98.0, 20.0) {
                    click(self);
                    if let Some(book) = self.book.as_mut() {
                        book.begin_signing();
                    }
                } else if inside(w / 2.0 + 2.0, 196.0, 98.0, 20.0) {
                    click(self);
                    self.save_book(false);
                } else if inside(left + 36.0, 32.0, 114.0, 14.0 * 9.0) {
                    self.book_page_click(left + 36.0, 32.0, mx, my, double);
                }
            }
            Some(BookScreen::View {
                pages,
                page,
                lectern,
            }) => {
                let (page, count, lectern) = (*page, pages.len(), *lectern);
                // The text's click event first, then the buttons.
                if let Some(link) = pages.get(page).and_then(|t| self.book_link_at(t, left)) {
                    self.book_follow_link(link, count, lectern);
                } else if page + 1 < count && inside(left + 116.0, 159.0, 23.0, 13.0) {
                    self.turn_page(true);
                } else if page > 0 && inside(left + 43.0, 159.0, 23.0, 13.0) {
                    self.turn_page(false);
                } else if let Some(key) = lectern.filter(|_| self.may_build()) {
                    if inside(w / 2.0 - 100.0, 196.0, 98.0, 20.0) {
                        click(self);
                        self.close_book();
                    } else if inside(w / 2.0 + 2.0, 196.0, 98.0, 20.0) {
                        click(self);
                        self.take_lectern_book(key);
                    }
                } else if inside((w - 200.0) / 2.0, 196.0, 200.0, 20.0) {
                    click(self);
                    self.close_book();
                }
            }
            None => {}
        }
    }
    pub(super) fn draw_book(&self, batch: &mut HudBatch) {
        let Some(book) = &self.book else { return };
        let atlas = &self.gui.atlas;
        let w = batch.width;
        let (mx, my) = self.cursor;
        let hover =
            |x: f32, y: f32, bw: f32, bh: f32| mx >= x && my >= y && mx < x + bw && my < y + bh;
        self.render_transparent_background("book", batch);
        let left = ((w - 192.0) / 2.0).floor();
        let top = 2.0;
        if let Some(mut r) = atlas.sprites.get("gui/book").copied() {
            let (du, dv) = ((r.u1 - r.u0) / r.w as f32, (r.v1 - r.v0) / r.h as f32);
            r.u1 = r.u0 + 192.0 * du;
            r.v1 = r.v0 + 192.0 * dv;
            batch.rect_uv(left, top, 192.0, 192.0, r, u32::MAX);
        }
        // The page, or the signing title on it: a field to a controller, which opens its keyboard.
        if matches!(book, BookScreen::Edit { .. }) {
            widgets::target_as(
                [
                    left + BOOK_TEXT[0],
                    top + BOOK_TEXT[1],
                    BOOK_TEXT[2],
                    BOOK_TEXT[3],
                ],
                widgets::Kind::Field,
            );
        }
        let tr = |key: &str, fallback: &str| self.translate(key, fallback);
        let page_buttons = |batch: &mut HudBatch, back: bool, forward: bool| {
            for (show, x, name) in [
                (forward, 116.0, "page_forward"),
                (back, 43.0, "page_backward"),
            ] {
                if show {
                    let sprite = if hover(left + x, top + 157.0, 23.0, 13.0) {
                        format!("gui/sprites/widget/{name}_highlighted")
                    } else {
                        format!("gui/sprites/widget/{name}")
                    };
                    batch.sprite(atlas, &sprite, left + x, top + 157.0);
                }
            }
        };
        let indicator = |batch: &mut HudBatch, page: usize, count: usize| {
            // BookViewScreen.getPageNumberMessage: 26.3's en_us template is
            // positional ("Page %1$s of %2$s").
            let text = horizon_protocol::text::format_java(
                &tr("book.pageIndicator", "Page %1$s of %2$s"),
                &[(page + 1).to_string(), count.max(1).to_string()],
            );
            let tw = HudBatch::text_width(atlas, &text);
            batch.text(atlas, &text, left + 148.0 - tw, top + 16.0, BLACK, false);
        };
        // PageButton: a button named for where it turns, there while it can;
        // the view adds the forward one first, the editor the back one.
        let page_narratables = |back: bool, forward: bool, view: bool| {
            let back = (back, 43.0, "book.page_button.previous");
            let forward = (forward, 116.0, "book.page_button.next");
            let order = if view {
                [forward, back]
            } else {
                [back, forward]
            };
            for (show, x, key) in order {
                if show {
                    widgets::narratable(|| {
                        let rect = [left + x, top + 157.0, 23.0, 13.0];
                        Narratable::button(screen_info::tr(key, &[]), rect, true)
                            .hovered(hover(rect[0], rect[1], rect[2], rect[3]))
                            .order(i32::from(view))
                    });
                }
            }
        };
        let button = |batch: &mut HudBatch, x: f32, bw: f32, label: &str, active: bool| {
            ui::button(
                batch,
                atlas,
                x,
                196.0,
                bw,
                20.0,
                &Text::literal(label),
                active,
                hover(x, 196.0, bw, 20.0),
            );
        };
        let action_button = |batch: &mut HudBatch, action, x: f32, bw: f32, label: &str| {
            ui::action_button(
                batch,
                atlas,
                action,
                x,
                196.0,
                bw,
                20.0,
                &Text::literal(label),
                true,
                hover(x, 196.0, bw, 20.0),
            );
        };
        match book {
            BookScreen::Edit {
                signing: true,
                title,
                title_edit,
                ..
            } => {
                // BookSignScreen: its title, and the title box focused.
                widgets::begin_screen(|| {
                    ScreenInfo::new("book/sign", screen_info::tr("book.sign.title", &[]))
                });
                widgets::narratable(|| {
                    let rect = [((w - 114.0) / 2.0).floor() - 3.0, 50.0, 114.0, 20.0];
                    Narratable::edit_box(
                        screen_info::tr("book.sign.titlebox", &[]),
                        title_edit.value.clone(),
                        rect,
                    )
                    .focused(title_edit.focused)
                });
                let edit = tr("book.editTitle", "Enter Book Title:");
                let tw = HudBatch::text_width(atlas, &edit);
                batch.text(
                    atlas,
                    &edit,
                    left + 36.0 + (114.0 - tw) / 2.0,
                    34.0,
                    BLACK,
                    false,
                );
                // The title box: an unbordered, centred EditBox of 114 with
                // black text and no shadow; the value alone is centred, the
                // cursor follows it.
                let value = &title_edit.value;
                let x = left + 36.0;
                let tx = x + ((114.0 - HudBatch::text_width(atlas, value)) / 2.0).floor();
                batch.text(atlas, value, tx, 50.0, BLACK, false);
                if title_edit.cursor != title_edit.highlight {
                    let (a, b) = (
                        title_edit.cursor.min(title_edit.highlight),
                        title_edit.cursor.max(title_edit.highlight),
                    );
                    let pre: String = value.chars().take(a).collect();
                    let sel: String = value.chars().skip(a).take(b - a).collect();
                    let sx = tx + HudBatch::text_width(atlas, &pre);
                    batch.text_highlight(
                        atlas,
                        (sx, 49.0, HudBatch::text_width(atlas, &sel), 11.0),
                        true,
                    );
                }
                if title_edit.cursor_visible() {
                    let pre: String = value.chars().take(title_edit.cursor).collect();
                    batch.fill(
                        atlas,
                        tx + HudBatch::text_width(atlas, &pre),
                        49.0,
                        1.0,
                        9.0,
                        BLACK,
                    );
                }
                // BookSignScreen: `owner.getName()`, the account's name on a server.
                let owner = self
                    .remote
                    .as_ref()
                    .map_or(super::chat::PLAYER_NAME, |r| r.profile_name.as_str());
                let by = tr("book.byAuthor", "by %1$s").replace("%1$s", owner);
                let tw = HudBatch::text_width(atlas, &by);
                batch.text(
                    atlas,
                    &by,
                    left + 36.0 + (114.0 - tw) / 2.0,
                    60.0,
                    rgba(64, 64, 64, 255),
                    false,
                );
                let warning = tr(
                    "book.finalizeWarning",
                    "Note! When you sign the book, it will no longer be editable.",
                );
                for (i, line) in atlas
                    .font
                    .split(&Text::literal(&warning), 114.0)
                    .iter()
                    .enumerate()
                {
                    batch.text_styled(
                        atlas,
                        line,
                        left + 36.0,
                        82.0 + i as f32 * 9.0,
                        BLACK,
                        false,
                    );
                }
                button(
                    batch,
                    w / 2.0 - 100.0,
                    98.0,
                    &tr("book.finalizeButton", "Sign and Close"),
                    !title.trim().is_empty(),
                );
                action_button(
                    batch,
                    widgets::Action::Cancel,
                    w / 2.0 + 2.0,
                    98.0,
                    &tr("gui.cancel", "Cancel"),
                );
            }
            BookScreen::Edit {
                pages,
                page,
                page_edit,
                ..
            } => {
                widgets::begin_screen(|| {
                    screen_info::book_edit(*page, pages.len(), &screen_info::tr)
                });
                // The page's MultiLineEditBox (no message), focused, then the
                // page buttons.
                widgets::narratable(|| {
                    let rect = [((w - 114.0) / 2.0).floor() - 8.0, 28.0, 122.0, 134.0];
                    Narratable::edit_box("", pages[*page].clone(), rect).focused(page_edit.focused)
                });
                page_narratables(*page > 0, true, false);
                indicator(batch, *page, pages.len());
                let text = &pages[*page];
                let lines = self.book_lines(text);
                for (i, line) in lines.iter().take(LINES).enumerate() {
                    batch.text_styled(
                        atlas,
                        line,
                        left + 36.0,
                        top + 30.0 + i as f32 * 9.0,
                        BLACK,
                        false,
                    );
                }
                let rows = self.page_rows(text);
                let (sel_a, sel_b) = (
                    page_edit.cursor.min(page_edit.highlight),
                    page_edit.cursor.max(page_edit.highlight),
                );
                if sel_a != sel_b {
                    let chars: Vec<char> = text.chars().collect();
                    for (i, &(start, end)) in rows.iter().enumerate().take(LINES) {
                        let from = sel_a.max(start);
                        let to = sel_b.min(end);
                        if from >= to {
                            continue;
                        }
                        let pre: String = chars[start..from].iter().collect();
                        let sel: String = chars[from..to].iter().collect();
                        batch.text_highlight(
                            atlas,
                            (
                                left + 36.0 + atlas.font.width(&pre),
                                top + 29.0 + i as f32 * 9.0,
                                atlas.font.width(&sel),
                                9.0,
                            ),
                            true,
                        );
                    }
                }
                if page_edit.cursor_visible() {
                    let (x, row) = self.caret_on(text, &rows, page_edit.cursor);
                    batch.fill(
                        atlas,
                        left + 36.0 + x,
                        top + 29.0 + row as f32 * 9.0,
                        1.0,
                        9.0,
                        BLACK,
                    );
                }
                page_buttons(batch, *page > 0, true);
                button(
                    batch,
                    w / 2.0 - 100.0,
                    98.0,
                    &tr("book.signButton", "Sign"),
                    true,
                );
                action_button(
                    batch,
                    widgets::Action::Done,
                    w / 2.0 + 2.0,
                    98.0,
                    &tr("gui.done", "Done"),
                );
            }
            BookScreen::View {
                pages,
                page,
                lectern,
            } => {
                widgets::begin_screen(|| {
                    let text = pages.get(*page).map(Text::plain).unwrap_or_default();
                    screen_info::book_view(*page, pages.len(), &text, &screen_info::tr)
                });
                // createMenuControls comes before createPageControlButtons
                // (forward, then back).
                page_narratables(*page > 0, *page + 1 < pages.len(), true);
                indicator(batch, *page, pages.len());
                if let Some(text) = pages.get(*page) {
                    for (i, line) in self.page_lines(text).iter().take(LINES).enumerate() {
                        batch.text_styled(
                            atlas,
                            line,
                            left + 36.0,
                            top + 30.0 + i as f32 * 9.0,
                            BLACK,
                            false,
                        );
                    }
                }
                page_buttons(batch, *page > 0, *page + 1 < pages.len());
                // LecternScreen.createMenuControls: Done and Take Book only with
                // build permission, else BookViewScreen's wide Done.
                if lectern.is_some() && self.may_build() {
                    action_button(
                        batch,
                        widgets::Action::Done,
                        w / 2.0 - 100.0,
                        98.0,
                        &tr("gui.done", "Done"),
                    );
                    button(
                        batch,
                        w / 2.0 + 2.0,
                        98.0,
                        &tr("lectern.take_book", "Take Book"),
                        true,
                    );
                } else {
                    action_button(
                        batch,
                        widgets::Action::Done,
                        (w - 200.0) / 2.0,
                        200.0,
                        &tr("gui.done", "Done"),
                    );
                }
                // HoveredTextEffects.TOOLTIP_AND_CURSOR: a clickable word
                // asks for the hand, and a show_text hover shows its tooltip,
                // as the chat does.
                if let Some(style) = pages.get(*page).and_then(|t| self.book_style_at(t, left)) {
                    if let Some(cursor) = link_cursor(&style) {
                        horizon_client::widgets::request_cursor(cursor);
                    }
                    if let Some(hover) = style.hover {
                        let lines = self.wrapped_rows(&hover, (w as i32 / 2).max(200));
                        self.draw_tooltip_lines(batch, &lines, mx, my);
                    }
                }
            }
        }
    }
}
/// `LecternBlock.getAnalogOutputSignal` and
/// `LecternBlockEntity.getRedstoneSignal`: the state's `has_book` gates the
/// answer, and an entity with no book reads 14 (a page count of 0 is a page
/// progress of 1.0 with no book to add).
pub(super) fn lectern_signal(
    world: &horizon_client::world::World,
    registry: &horizon_world::registry::BlockRegistry,
    state: BlockStateId,
    pos: BlockPos,
) -> Option<i32> {
    let s = registry.state(state)?;
    if s.name != "minecraft:lectern" {
        return None;
    }
    if s.properties.get("has_book").is_none_or(|v| v != "true") {
        return Some(0);
    }
    match world.block_entities.get(&(pos.x, pos.y, pos.z)) {
        Some(BlockEntity::Lectern(l)) => Some(l.book.as_ref().map_or(14, |b| {
            let data = b.data();
            let count = data
                .and_then(|d| d.written.as_ref().map(|w| w.pages.len()))
                .or_else(|| data.map(|d| d.writable_pages.len()))
                .unwrap_or(0)
                .max(1);
            let f = if count > 1 {
                l.page as f32 / (count - 1) as f32
            } else {
                1.0
            };
            // `LecternBlockEntity.hasBook`: a `written_book_content` or
            // `writable_book_content` component, not just any stack. A book
            // item carries its content by default, so its id counts too.
            let has_book = matches!(
                &*b.item,
                "minecraft:writable_book" | "minecraft:written_book"
            ) || data
                .is_some_and(|d| d.written.is_some() || !d.writable_pages.is_empty());
            (f * 14.0).floor() as i32 + i32::from(has_book)
        })),
        _ => None,
    }
}

/// `HoveredTextEffects.TOOLTIP_AND_CURSOR`: only a word with a click event asks
/// for the pointing hand over a book's page; a hover event alone shows its
/// tooltip.
fn link_cursor(style: &horizon_client::text::Style) -> Option<horizon_client::widgets::Cursor> {
    style
        .click
        .is_some()
        .then_some(horizon_client::widgets::Cursor::PointingHand)
}

#[cfg(test)]
mod title_box_tests {
    use super::book_title_box;

    /// `BookSignScreen` sets its 15 character title box centred and unbordered.
    #[test]
    fn the_sign_title_box_is_centred() {
        let edit = book_title_box();
        assert!(edit.centered);
        assert!(!edit.bordered);
        assert_eq!(edit.max_length, 15);
    }
}
#[cfg(test)]
mod lectern_books_tag_tests {
    use super::super::native_machines::tests::server_game;
    use horizon_client::{containers::BlockEntity, items::ItemStack};

    /// `LecternBlock.useItemOn` checks `ItemTags.LECTERN_BOOKS`; a world's
    /// data pack adds its own book to that tag, and the lectern takes it.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_packs_lectern_books_entry_goes_onto_the_lectern() {
        let mut game = server_game();
        let dir =
            std::env::temp_dir().join(format!("horizon-lectern-books-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let tags = dir.join("datapacks/books/data/minecraft/tags/item");
        std::fs::create_dir_all(&tags).unwrap();
        std::fs::write(
            dir.join("datapacks/books/pack.mcmeta"),
            r#"{"pack":{"description":"fixture"}}"#,
        )
        .unwrap();
        std::fs::write(
            tags.join("lectern_books.json"),
            r#"{"values":["minecraft:written_book","minecraft:writable_book","minecraft:book"]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("datapacks.json"),
            r#"{"enabled":["file/books"],"disabled":[]}"#,
        )
        .unwrap();
        game.world_root = dir;
        game.world_tables = game.build_world_tables(false, &[]).unwrap().0;

        let state = game
            .pack
            .assets
            .registry
            .default_state("minecraft:lectern")
            .unwrap();
        let key = (8, 64, 8);
        game.world.set(key.0, key.1, key.2, state).unwrap();

        // A book the pack put in the tag is placed; one it did not is not.
        let stone = ItemStack::new("minecraft:stone", 1);
        assert!(!game.use_lectern([8, 64, 8], Some(&stone)));
        let book = ItemStack::new("minecraft:book", 1);
        assert!(
            game.use_lectern([8, 64, 8], Some(&book)),
            "the pack's `#minecraft:lectern_books` entry was refused"
        );
        let Some(BlockEntity::Lectern(lectern)) = game.world.block_entities.get(&key) else {
            panic!("no lectern block entity");
        };
        assert_eq!(
            lectern.book.as_ref().map(|b| &*b.item),
            Some("minecraft:book")
        );
    }
}

#[cfg(test)]
#[path = "lectern_tests.rs"]
mod lectern_tests;

#[cfg(test)]
mod lectern_signal_tests {
    use super::super::native_machines::tests::server_game;
    use super::lectern_signal;
    use horizon_client::{
        containers::{BlockEntity, Lectern},
        item_data::{StackData, WrittenBook},
        items::ItemStack,
    };
    use horizon_core::position::BlockPos;
    use horizon_world::level::LevelAccess;
    use horizon_world::registry::BlockStateId;

    fn signal(
        game: &mut super::Game<'_>,
        has_book: bool,
        book: Option<ItemStack>,
        page: i32,
    ) -> i32 {
        let key = (8, 64, 8);
        let registry = game.pack.assets.registry;
        let default = registry.default_state("minecraft:lectern").unwrap();
        let mut state = registry.state(default).unwrap().clone();
        state
            .properties
            .insert("has_book".into(), has_book.to_string());
        let state: BlockStateId = registry.id(&state).unwrap();
        game.world.set(key.0, key.1, key.2, state).unwrap();
        game.world
            .block_entities
            .insert(key, BlockEntity::Lectern(Lectern { book, page }));
        let pos = BlockPos::new(key.0, key.1, key.2);
        lectern_signal(&game.world, registry, state, pos).unwrap()
    }

    /// `LecternBlock.getAnalogOutputSignal` reads the block entity only while
    /// the state's `HAS_BOOK` is true.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_lectern_without_the_has_book_state_feeds_no_comparator() {
        let mut game = server_game();
        let key = (8, 64, 8);
        let state = game
            .pack
            .assets
            .registry
            .default_state("minecraft:lectern")
            .unwrap();
        game.world.set(key.0, key.1, key.2, state).unwrap();
        game.world.block_entities.insert(
            key,
            BlockEntity::Lectern(Lectern {
                book: Some(ItemStack::new("minecraft:writable_book", 1)),
                page: 0,
            }),
        );
        let signal = |game: &mut super::Game<'_>, state| {
            game.with_level(|_, level, _| {
                level.block_entity_analog_signal(
                    BlockPos::new(key.0, key.1, key.2),
                    state,
                    horizon_core::geometry::Direction::North,
                )
            })
        };
        assert_eq!(
            signal(&mut game, state),
            0,
            "the block state says the lectern holds no book"
        );
        let registry = game.pack.assets.registry;
        let mut with_book = registry.state(state).unwrap().clone();
        with_book
            .properties
            .insert("has_book".into(), "true".into());
        let with_book = registry.id(&with_book).unwrap();
        game.world.set(key.0, key.1, key.2, with_book).unwrap();
        assert_eq!(
            signal(&mut game, with_book),
            15,
            "with HAS_BOOK the book's page signal"
        );
    }

    /// `LecternBlock.getAnalogOutputSignal` asks for the state's `HAS_BOOK`
    /// first, so a lectern without it reads 0 whatever its block entity holds;
    /// with it, `LecternBlockEntity.getRedstoneSignal` is `floor(page /
    /// (pageCount - 1) * 14) + (hasBook() ? 1 : 0)`. `hasBook()` asks for a
    /// book-content component, so a stack that only sits in the lectern (a
    /// non-book item a command left in `Book`) reads 14, not the book's 15.
    /// An entity with no book at all reads 14 too: a page count of 0 is a page
    /// progress of 1.0 with no book to add.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_lecterns_signal_adds_one_only_for_a_book() {
        let mut game = server_game();
        let writable = Some(ItemStack::new("minecraft:writable_book", 1));
        let mut written = ItemStack::new("minecraft:written_book", 1);
        written.extra = Some(Box::new(StackData {
            written: Some(WrittenBook {
                title: "T".into(),
                author: "A".into(),
                pages: vec!["one".into(), "two".into()],
                ..Default::default()
            }),
            ..Default::default()
        }));
        // `has_book=false`: no signal, whatever the block entity holds.
        assert_eq!(signal(&mut game, false, None, 0), 0);
        assert_eq!(
            signal(&mut game, false, writable.clone(), 0),
            0,
            "state denies"
        );
        // `has_book=true`, an entity with no book: page progress 1.0.
        assert_eq!(signal(&mut game, true, None, 0), 14, "no book");
        let stone = Some(ItemStack::new("minecraft:stone", 1));
        assert_eq!(signal(&mut game, true, stone, 0), 14, "no book content");
        // `writable_book_content` is a writable book's default component.
        assert_eq!(signal(&mut game, true, writable, 0), 15, "a book");
        assert_eq!(
            signal(&mut game, true, Some(written.clone()), 0),
            1,
            "page 0"
        );
        assert_eq!(signal(&mut game, true, Some(written), 1), 15, "last page");
    }
}
impl BookScreen {
    /// `BookViewScreen.isPauseScreen`: the class inherits `Screen`'s true, so
    /// a book read in hand stops a single-player world. `LecternScreen`
    /// overrides it false, so the same pages read on a lectern leave the
    /// world ticking (its menu is an `AbstractContainerScreen`, which does
    /// not pause either).
    pub(super) fn pauses_world(&self) -> bool {
        !matches!(
            self,
            Self::View {
                lectern: Some(_),
                ..
            }
        )
    }
}

#[cfg(test)]
mod pause_tests {
    use super::BookScreen;
    use horizon_client::{text::Text, ui};

    fn view(lectern: Option<(i32, i32, i32)>) -> BookScreen {
        BookScreen::View {
            pages: Vec::<Text>::new(),
            page: 0,
            lectern,
        }
    }

    #[test]
    fn a_lecterns_book_does_not_pause_the_world() {
        // BookViewScreen keeps Screen.isPauseScreen's true; LecternScreen
        // overrides it false although both read the same pages.
        assert!(view(None).pauses_world());
        assert!(!view(Some((10, 64, -3))).pauses_world());
    }

    #[test]
    fn a_book_being_edited_or_signed_pauses() {
        // BookEditScreen and BookSignScreen do not override isPauseScreen.
        let edit = |signing| BookScreen::Edit {
            slot: 0,
            pages: Vec::new(),
            page: 0,
            signing,
            title: String::new(),
            title_edit: ui::EditBox::new(0.0, 0.0, 0.0, 0.0),
            page_edit: ui::EditBox::new(0.0, 0.0, 0.0, 0.0),
        };
        assert!(edit(false).pauses_world());
        assert!(edit(true).pauses_world());
    }
}

#[cfg(test)]
mod lectern_reach_tests {
    use super::super::native_machines::tests::server_game;
    use horizon_client::{
        containers::{BlockEntity, Lectern},
        items::ItemStack,
        motion::PlayerMotion,
    };
    use horizon_core::geometry::Vec3;

    /// `LecternMenu.stillValid` -> `Container.stillValidBlockEntity`: the open
    /// lectern screen closes once the player is out of the interaction range
    /// (with vanilla's four-block buffer), as the guest path already does.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_lectern_screen_closes_out_of_reach() {
        let mut game = server_game();
        game.player = Some(
            PlayerMotion::new(game.motion, Vec3::new(8.5, 64.0, 8.5)).expect("a player position"),
        );
        let key = (8, 64, 8);
        let state = game
            .pack
            .assets
            .registry
            .default_state("minecraft:lectern")
            .unwrap();
        game.world.set(key.0, key.1, key.2, state).unwrap();
        game.world.block_entities.insert(
            key,
            BlockEntity::Lectern(Lectern {
                book: Some(ItemStack::new("minecraft:writable_book", 1)),
                page: 0,
            }),
        );
        assert!(game.open_lectern([8, 64, 8]));
        assert!(game.book.is_some(), "the lectern screen opened");

        game.tick_lecterns();
        assert!(game.book.is_some(), "the player is within reach");

        game.player.as_mut().unwrap().position = Vec3::new(30.0, 64.0, 10.5);
        game.tick_lecterns();
        assert!(
            game.book.is_none(),
            "the lectern screen stayed open out of reach"
        );
    }
}

#[cfg(test)]
mod sign_tests {
    use super::{BookScreen, book_page_box, book_title_box};

    /// `BookSignScreen`'s constructor sets its title box to `titleValue`, what
    /// Cancel kept; reopening the sign screen shows the title typed before,
    /// not an empty box beside a stale `title`.
    #[test]
    fn the_sign_screen_opens_with_the_title_it_kept() {
        let mut book = BookScreen::Edit {
            slot: 0,
            pages: vec!["page".into()],
            page: 0,
            signing: false,
            title: "My Book".into(),
            title_edit: book_title_box(),
            page_edit: book_page_box(),
        };
        assert_eq!(book_title_value(&book), "");
        book.begin_signing();
        let BookScreen::Edit {
            signing,
            title,
            title_edit,
            ..
        } = &book
        else {
            panic!("the book stays open");
        };
        assert!(signing);
        assert_eq!(title.as_str(), "My Book");
        assert_eq!(title_edit.value, "My Book");
        assert_eq!(title_edit.cursor, 7);
    }

    fn book_title_value(book: &BookScreen) -> String {
        match book {
            BookScreen::Edit { title_edit, .. } => title_edit.value.clone(),
            BookScreen::View { .. } => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::native_machines::tests::server_game;

    /// `LecternBlock.resetBookState`: the book coming off a lectern clears
    /// `has_book` and posts `block_change` at it.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_lectern_book_removal_posts_block_change() {
        let mut game = server_game();
        let key = (2, 4, 3);
        let state = game
            .pack
            .assets
            .registry
            .default_state("minecraft:lectern")
            .unwrap();
        game.world.set(key.0, key.1, key.2, state).unwrap();
        game.world.block_entities.put(
            key,
            BlockEntity::Lectern(Lectern {
                book: Some(ItemStack::new("minecraft:written_book", 1)),
                page: 0,
            }),
        );
        game.set_lectern_property(key, "has_book", true);
        super::super::sculk::TEST_POSTED_EVENTS.with(|e| *e.borrow_mut() = Some(Vec::new()));

        game.set_lectern_has_book(key, false, None);

        let has_book = game
            .pack
            .assets
            .registry
            .state(game.world.block(key.0, key.1, key.2))
            .and_then(|s| s.properties.get("has_book").cloned());
        assert_eq!(
            has_book.as_deref(),
            Some("false"),
            "the lectern has no book"
        );
        super::super::sculk::TEST_POSTED_EVENTS.with(|e| {
            assert!(
                e.borrow().as_ref().unwrap().contains(&(
                    "minecraft:block_change",
                    [2.5, 4.5, 3.5],
                    None
                )),
                "{:?}",
                e.borrow()
            );
        });
    }
}

#[cfg(test)]
mod link_cursor_tests {
    use super::link_cursor;
    use horizon_client::text::{ClickEvent, Style, Text};
    use horizon_client::widgets::Cursor;

    /// `HoveredTextEffects.TOOLTIP_AND_CURSOR`: a clickable word asks for the
    /// pointing hand; a hover event alone only shows its tooltip.
    #[test]
    fn only_a_clickable_book_word_asks_for_the_hand() {
        let link = Style {
            click: Some(ClickEvent::OpenUrl("https://example.com".into())),
            ..Style::default()
        };
        let hover_only = Style {
            hover: Some(Box::new(Text::literal("tip"))),
            ..Style::default()
        };
        assert_eq!(link_cursor(&link), Some(Cursor::PointingHand));
        assert_eq!(link_cursor(&hover_only), None);
    }
}
