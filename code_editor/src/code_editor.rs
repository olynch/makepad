use {
    crate::{
        layout::{BlockElement, WrappedElement},
        selection::Affinity,
        settings::Settings,
        state::Session,
        str::StrExt,
        text::Position,
        token::TokenKind,
        Line, Selection, Token,
    },
    makepad_widgets::*,
    std::{mem, slice::Iter},
};

live_design! {
    import makepad_draw::shader::std::*;
    import makepad_widgets::theme_desktop_dark::*;

    TokenColors = {{TokenColors}} {
        unknown: #808080,
        branch_keyword: #C485BE,
        comment: #638D54,
        constant: #CC917B,
        identifier: #D4D4D4,
        loop_keyword: #FF8C00,
        number: #B6CEAA,
        other_keyword: #5B9BD3,
        punctuator: #D4D4D4,
        string: #CC917B,
        typename: #56C9B1;
        whitespace: #6E6E6E,
    }

    DrawIndentGuide = {{DrawIndentGuide}} {
        fn pixel(self) -> vec4 {
            let thickness = 0.8 + self.dpi_dilate * 0.5;
            let sdf = Sdf2d::viewport(self.pos * self.rect_size);
            sdf.move_to(1., -1.);
            sdf.line_to(1., self.rect_size.y + 1.);
            return sdf.stroke(self.color, thickness);
        }
    }

    DrawSelection = {{DrawSelection}} {
        uniform gloopiness: 8.0
        uniform border_radius: 2.0

        fn vertex(self) -> vec4 {
            let clipped: vec2 = clamp(
                self.geom_pos * vec2(self.rect_size.x + 16., self.rect_size.y) + self.rect_pos - vec2(8., 0.),
                self.draw_clip.xy,
                self.draw_clip.zw
            );
            self.pos = (clipped - self.rect_pos) / self.rect_size;
            return self.camera_projection * (self.camera_view * (
                self.view_transform * vec4(clipped.x, clipped.y, self.draw_depth + self.draw_zbias, 1.)
            ));
        }

        fn pixel(self) -> vec4 {
            let sdf = Sdf2d::viewport(self.rect_pos + self.pos * self.rect_size);
            sdf.box(
                self.rect_pos.x,
                self.rect_pos.y,
                self.rect_size.x,
                self.rect_size.y,
                self.border_radius
            );
            if self.prev_w > 0.0 {
                sdf.box(
                    self.prev_x,
                    self.rect_pos.y - self.rect_size.y,
                    self.prev_w,
                    self.rect_size.y,
                    self.border_radius
                );
                sdf.gloop(self.gloopiness);
            }
            if self.next_w > 0.0 {
                sdf.box(
                    self.next_x,
                    self.rect_pos.y + self.rect_size.y,
                    self.next_w,
                    self.rect_size.y,
                    self.border_radius
                );
                sdf.gloop(self.gloopiness);
            }
            return sdf.fill(#08f8);
        }
    }

    CodeEditor = {{CodeEditor}} {
        width: Fill,
        height: Fill,
        margin: 0,
        scroll_bars: <ScrollBars>{}
        draw_bg:{
            draw_depth: 0.0,
            color:#3
        }
        draw_gutter: {
            draw_depth: 1.0,
            text_style: <THEME_FONT_CODE> {},
            color: #C0C0C0,
        }
        draw_text: {
            draw_depth: 1.0,
            text_style: <THEME_FONT_CODE> {}
        }
        draw_indent_guide: {
            draw_depth: 2.0,
            color: #C0C0C0,
        }
        draw_selection: {
            draw_depth: 3.0,
        }
        draw_cursor: {
            draw_depth: 4.0,
            color: #C0C0C0,
        }
    }
}

#[derive(Live)]
pub struct CodeEditor {
    #[walk]
    walk: Walk,
    #[live]
    scroll_bars: ScrollBars,
    #[rust]
    draw_state: DrawStateWrap<Walk>,
    #[live]
    draw_gutter: DrawText,
    #[live]
    draw_text: DrawText,
    #[live]
    token_colors: TokenColors,
    #[live]
    draw_indent_guide: DrawIndentGuide,
    #[live]
    draw_selection: DrawSelection,
    #[live]
    draw_cursor: DrawColor,
    #[live]
    draw_bg: DrawColor,

    #[rust]
    cell_size: DVec2,
    #[rust]
    gutter_rect: Rect,
    #[rust]
    viewport_rect: Rect,
    #[rust]
    line_start: usize,
    #[rust]
    line_end: usize,
}

impl LiveHook for CodeEditor {
    fn before_live_design(cx: &mut Cx) {
        register_widget!(cx, CodeEditor)
    }
}

impl Widget for CodeEditor {
    fn redraw(&mut self, cx: &mut Cx) {
        self.scroll_bars.redraw(cx);
    }

    fn handle_widget_event_with(
        &mut self,
        _cx: &mut Cx,
        _event: &Event,
        _dispatch_action: &mut dyn FnMut(&mut Cx, WidgetActionItem),
    ) {
    }

    fn walk(&mut self, _cx: &mut Cx) -> Walk {
        self.walk
    }

    fn draw_walk_widget(&mut self, cx: &mut Cx2d, walk: Walk) -> WidgetDraw {
        if self.draw_state.begin(cx, walk) {
            return WidgetDraw::hook_above();
        }
        self.draw_state.end();
        WidgetDraw::done()
    }
}

#[derive(Clone, PartialEq, WidgetRef)]
pub struct CodeEditorRef(WidgetRef);

impl CodeEditor {
    pub fn draw(&mut self, cx: &mut Cx2d, session: &mut Session) {
        self.cell_size =
            self.draw_text.text_style.font_size * self.draw_text.get_monospace_base(cx);
        let walk = self.draw_state.get().unwrap();
        self.scroll_bars.begin(cx, walk, Layout::default());
        let turtle_rect = cx.turtle().rect();
        let gutter_width = (session
            .document()
            .as_text()
            .as_lines()
            .len()
            .to_string()
            .column_count()
            + 1) as f64
            * self.cell_size.x;
        self.gutter_rect = Rect {
            pos: turtle_rect.pos,
            size: DVec2 {
                x: gutter_width,
                y: turtle_rect.size.y,
            },
        };
        self.viewport_rect = Rect {
            pos: DVec2 {
                x: turtle_rect.pos.x + gutter_width,
                y: turtle_rect.pos.y,
            },
            size: DVec2 {
                x: turtle_rect.size.x - gutter_width,
                y: turtle_rect.size.y,
            },
        };

        let pad_left_top = dvec2(10., 10.);
        self.gutter_rect.pos += pad_left_top;
        self.gutter_rect.size -= pad_left_top;
        self.viewport_rect.pos += pad_left_top;
        self.viewport_rect.size -= pad_left_top;
        
        session.handle_changes();
        session.set_wrap_column(Some(
            (self.viewport_rect.size.x / self.cell_size.x) as usize,
        ));
        let scroll_pos = self.scroll_bars.get_scroll_pos();
        self.line_start = session
            .layout()
            .find_first_line_ending_after_y(scroll_pos.y / self.cell_size.y);
        self.line_end = session.layout().find_first_line_starting_after_y(
            (scroll_pos.y + self.viewport_rect.size.y) / self.cell_size.y,
        );

        self.draw_bg.draw_abs(cx, cx.turtle().unscrolled_rect());
        self.draw_gutter(cx, session);
        self.draw_text_layer(cx, session);
        self.draw_indent_guide_layer(cx, session);
        self.draw_selection_layer(cx, session);

        cx.turtle_mut().set_used(
            session.layout().width() * self.cell_size.x,
            session.layout().height() * self.cell_size.y,
        );
        self.scroll_bars.end(cx);
        if session.update_folds() {
            cx.redraw_all();
        }
    }

    pub fn handle_event(
        &mut self,
        cx: &mut Cx,
        event: &Event,
        session: &mut Session,
    ) -> Vec<CodeEditorAction> {
        let mut a = Vec::new();
        self.handle_event_with(cx, event, session, &mut |_, v| a.push(v));
        a
    }

    pub fn handle_event_with(
        &mut self,
        cx: &mut Cx,
        event: &Event,
        session: &mut Session,
        dispatch_action: &mut dyn FnMut(&mut Cx, CodeEditorAction),
    ) {
        session.handle_changes();
        self.scroll_bars.handle_event_with(cx, event, &mut |cx, _| {
            cx.redraw_all();
        });

        match event.hits(cx, self.scroll_bars.area()) {
            Hit::KeyDown(KeyEvent {
                key_code: KeyCode::Escape,
                ..
            }) => {
                session.fold();
                cx.redraw_all();
            }
            Hit::KeyUp(KeyEvent {
                key_code: KeyCode::Escape,
                ..
            }) => {
                session.unfold();
                cx.redraw_all();
            }
            Hit::KeyDown(KeyEvent {
                key_code: KeyCode::ArrowLeft,
                modifiers: KeyModifiers { shift, .. },
                ..
            }) => {
                session.move_left(!shift);
                cx.redraw_all();
            }
            Hit::KeyDown(KeyEvent {
                key_code: KeyCode::ArrowRight,
                modifiers: KeyModifiers { shift, .. },
                ..
            }) => {
                session.move_right(!shift);
                cx.redraw_all();
            }
            Hit::KeyDown(KeyEvent {
                key_code: KeyCode::ArrowUp,
                modifiers: KeyModifiers { shift, .. },
                ..
            }) => {
                session.move_up(!shift);
                cx.redraw_all();
            }
            Hit::KeyDown(KeyEvent {
                key_code: KeyCode::ArrowDown,
                modifiers: KeyModifiers { shift, .. },
                ..
            }) => {
                session.move_down(!shift);
                cx.redraw_all();
            }
            Hit::TextInput(TextInputEvent { ref input, .. }) if input.len() > 0 => {
                session.insert(input.into());
                cx.redraw_all();
                dispatch_action(cx, CodeEditorAction::TextDidChange);
            }
            Hit::KeyDown(KeyEvent {
                key_code: KeyCode::ReturnKey,
                ..
            }) => {
                session.enter();
                cx.redraw_all();
                dispatch_action(cx, CodeEditorAction::TextDidChange);
            }
            Hit::KeyDown(KeyEvent {
                key_code: KeyCode::Tab,
                ..
            }) => {
                session.tab();
                cx.redraw_all();
                dispatch_action(cx, CodeEditorAction::TextDidChange);
            }
            Hit::KeyDown(KeyEvent {
                key_code: KeyCode::Delete,
                ..
            }) => {
                session.delete();
                cx.redraw_all();
                dispatch_action(cx, CodeEditorAction::TextDidChange);
            }
            Hit::KeyDown(KeyEvent {
                key_code: KeyCode::Backspace,
                ..
            }) => {
                session.backspace();
                cx.redraw_all();
                dispatch_action(cx, CodeEditorAction::TextDidChange);
            }

            Hit::KeyDown(KeyEvent {
                key_code: KeyCode::RBracket,
                modifiers: KeyModifiers { logo: true, .. },
                ..
            }) => {
                session.indent();
                cx.redraw_all();
                dispatch_action(cx, CodeEditorAction::TextDidChange);
            }
            Hit::KeyDown(KeyEvent {
                key_code: KeyCode::LBracket,
                modifiers: KeyModifiers { logo: true, .. },
                ..
            }) => {
                session.outdent();
                cx.redraw_all();
                dispatch_action(cx, CodeEditorAction::TextDidChange);
            }
            Hit::TextCopy(ce) => {
                *ce.response.borrow_mut() = Some(session.copy());
            }
            Hit::TextCut(ce) => {
                *ce.response.borrow_mut() = Some(session.copy());
                session.delete();
                cx.redraw_all();
            }
            Hit::KeyDown(KeyEvent {
                key_code: KeyCode::KeyZ,
                modifiers:
                    KeyModifiers {
                        logo: true,
                        shift: false,
                        ..
                    },
                ..
            }) => {
                if session.undo() {
                    cx.redraw_all();
                    dispatch_action(cx, CodeEditorAction::TextDidChange);
                }
            }
            Hit::KeyDown(KeyEvent {
                key_code: KeyCode::KeyZ,
                modifiers:
                    KeyModifiers {
                        logo: true,
                        shift: true,
                        ..
                    },
                ..
            }) => {
                if session.redo() {
                    cx.redraw_all();
                    dispatch_action(cx, CodeEditorAction::TextDidChange);
                }
            }
            Hit::FingerDown(FingerDownEvent {
                abs,
                modifiers: KeyModifiers { alt, .. },
                ..
            }) => {
                cx.set_key_focus(self.scroll_bars.area());
                if let Some((cursor, affinity)) = self.pick(session, abs) {
                    if alt {
                        session.add_cursor(cursor, affinity);
                    } else {
                        session.set_cursor(cursor, affinity);
                    }
                    cx.redraw_all();
                }
            }
            Hit::FingerHoverIn(_) | Hit::FingerHoverOver(_) => {
                cx.set_cursor(MouseCursor::Text);
            },
            Hit::FingerMove(FingerMoveEvent { abs, .. }) => {
                cx.set_cursor(MouseCursor::Text);
                if let Some((cursor, affinity)) = self.pick(session, abs) {
                    session.move_to(cursor, affinity);
                    cx.redraw_all();
                }
            }
            _ => {}
        }
    }

    fn draw_gutter(&mut self, cx: &mut Cx2d, session: &Session) {
        let mut line_index = self.line_start;
        let mut origin_y = session.layout().line(self.line_start).y();
        for element in session
            .layout()
            .block_elements(self.line_start, self.line_end)
        {
            match element {
                BlockElement::Line { line, .. } => {
                    self.draw_gutter.font_scale = line.scale();
                    self.draw_gutter.draw_abs(
                        cx,
                        DVec2 {
                            x: 0.0,
                            y: origin_y,
                        } * self.cell_size
                            + self.gutter_rect.pos,
                        &format!("{}", line_index),
                    );
                    line_index += 1;
                    origin_y += line.height();
                }
                BlockElement::Widget(widget) => {
                    origin_y += widget.height;
                }
            }
        }
    }

    fn draw_text_layer(&mut self, cx: &mut Cx2d, session: &Session) {
        let mut origin_y = session.layout().line(self.line_start).y();
        for element in session
            .layout()
            .block_elements(self.line_start, self.line_end)
        {
            match element {
                BlockElement::Line { line, .. } => {
                    self.draw_text.font_scale = line.scale();
                    let mut token_iter = line.tokens().iter().copied();
                    let mut token_slot = token_iter.next();
                    let mut row_index = 0;
                    let mut column_index = 0;
                    for element in line.wrapped_elements() {
                        match element {
                            WrappedElement::Text {
                                is_inlay: false,
                                mut text,
                            } => {
                                while !text.is_empty() {
                                    let token = match token_slot {
                                        Some(token) => {
                                            if text.len() < token.len {
                                                token_slot = Some(Token {
                                                    len: token.len - text.len(),
                                                    kind: token.kind,
                                                });
                                                Token {
                                                    len: text.len(),
                                                    kind: token.kind,
                                                }
                                            } else {
                                                token_slot = token_iter.next();
                                                token
                                            }
                                        }
                                        None => Token {
                                            len: text.len(),
                                            kind: TokenKind::Unknown,
                                        },
                                    };
                                    let (text_0, text_1) = text.split_at(token.len);
                                    text = text_1;
                                    self.draw_text.color = match token.kind {
                                        TokenKind::Unknown => self.token_colors.unknown,
                                        TokenKind::BranchKeyword => {
                                            self.token_colors.branch_keyword
                                        }
                                        TokenKind::Comment => self.token_colors.comment,
                                        TokenKind::Constant => self.token_colors.constant,
                                        TokenKind::Identifier => self.token_colors.identifier,
                                        TokenKind::LoopKeyword => self.token_colors.loop_keyword,
                                        TokenKind::Number => self.token_colors.number,
                                        TokenKind::OtherKeyword => self.token_colors.other_keyword,
                                        TokenKind::Punctuator => self.token_colors.punctuator,
                                        TokenKind::String => self.token_colors.string,
                                        TokenKind::Typename => self.token_colors.typename,
                                        TokenKind::Whitespace => self.token_colors.whitespace,
                                    };
                                    for grapheme in text_0.graphemes() {
                                        let (x, y) = line
                                            .grid_to_normalized_position(row_index, column_index);
                                        self.draw_text.draw_abs(
                                            cx,
                                            DVec2 { x, y: origin_y + y } * self.cell_size
                                                + self.viewport_rect.pos,
                                            grapheme,
                                        );
                                        column_index += grapheme.column_count();
                                    }
                                }
                            }
                            WrappedElement::Text {
                                is_inlay: true,
                                text,
                            } => {
                                let (x, y) =
                                    line.grid_to_normalized_position(row_index, column_index);
                                self.draw_text.draw_abs(
                                    cx,
                                    DVec2 { x, y: origin_y + y } * self.cell_size
                                        + self.viewport_rect.pos,
                                    text,
                                );
                                column_index += text.column_count();
                            }
                            WrappedElement::Widget(widget) => {
                                column_index += widget.column_count;
                            }
                            WrappedElement::Wrap => {
                                column_index = line.wrap_indent_column_count();
                                row_index += 1;
                            }
                        }
                    }
                    origin_y += line.height();
                }
                BlockElement::Widget(widget) => {
                    origin_y += widget.height;
                }
            }
        }
    }

    fn draw_indent_guide_layer(&mut self, cx: &mut Cx2d<'_>, session: &Session) {
        let mut origin_y = session.layout().line(self.line_start).y();
        for element in session
            .layout()
            .block_elements(self.line_start, self.line_end)
        {
            let Settings {
                tab_column_count, ..
            } = **session.settings();
            match element {
                BlockElement::Line { line, .. } => {
                    for row_index in 0..line.row_count() {
                        for column_index in
                            (0..line.indent_column_count()).step_by(tab_column_count)
                        {
                            let (x, y) = line.grid_to_normalized_position(row_index, column_index);
                            self.draw_indent_guide.draw_abs(
                                cx,
                                Rect {
                                    pos: DVec2 { x, y: origin_y + y } * self.cell_size
                                        + self.viewport_rect.pos,
                                    size: DVec2 {
                                        x: 2.0,
                                        y: line.scale() * self.cell_size.y,
                                    },
                                },
                            );
                        }
                    }
                    origin_y += line.height();
                }
                BlockElement::Widget(widget) => {
                    origin_y += widget.height;
                }
            }
        }
    }

    fn draw_selection_layer(&mut self, cx: &mut Cx2d<'_>, session: &Session) {
        let mut active_selection = None;
        let selections = session.selections();
        let mut selections = selections.iter();
        while selections.as_slice().first().map_or(false, |selection| {
            selection.end().line_index < self.line_start
        }) {
            selections.next().unwrap();
        }
        if selections.as_slice().first().map_or(false, |selection| {
            selection.start().line_index < self.line_start
        }) {
            active_selection = Some(ActiveSelection {
                selection: *selections.next().unwrap(),
                start_x: 0.0,
            });
        }
        DrawSelectionLayer {
            code_editor: self,
            active_selection,
            selections,
        }
        .draw_selection_layer(cx, session)
    }

    fn pick(&self, session: &Session, position: DVec2) -> Option<(Position, Affinity)> {
        let position = (position - self.viewport_rect.pos) / self.cell_size;
        let mut line_index = session.layout().find_first_line_ending_after_y(position.y);
        let mut origin_y = session.layout().line(line_index).y();
        for block in session.layout().block_elements(line_index, line_index + 1) {
            match block {
                BlockElement::Line {
                    is_inlay: false,
                    line,
                } => {
                    let mut byte_index = 0;
                    let mut row_index = 0;
                    let mut column_index = 0;
                    for element in line.wrapped_elements() {
                        match element {
                            WrappedElement::Text {
                                is_inlay: false,
                                text,
                            } => {
                                for grapheme in text.graphemes() {
                                    let (start_x, y) =
                                        line.grid_to_normalized_position(row_index, column_index);
                                    let start_y = origin_y + y;
                                    let (end_x, _) = line.grid_to_normalized_position(
                                        row_index,
                                        column_index + grapheme.column_count(),
                                    );
                                    let end_y = start_y + line.scale();
                                    if (start_y..=end_y).contains(&position.y) {
                                        let mid_x = (start_x + end_x) / 2.0;
                                        if (start_x..=mid_x).contains(&position.x) {
                                            return Some((
                                                Position {
                                                    line_index,
                                                    byte_index,
                                                },
                                                Affinity::After,
                                            ));
                                        }
                                        if (mid_x..=end_x).contains(&position.x) {
                                            return Some((
                                                Position {
                                                    line_index,
                                                    byte_index: byte_index + grapheme.len(),
                                                },
                                                Affinity::Before,
                                            ));
                                        }
                                    }
                                    byte_index += grapheme.len();
                                    column_index += grapheme.column_count();
                                }
                            }
                            WrappedElement::Text {
                                is_inlay: true,
                                text,
                            } => {
                                let (start_x, y) =
                                    line.grid_to_normalized_position(row_index, column_index);
                                let start_y = origin_y + y;
                                let (end_x, _) = line.grid_to_normalized_position(
                                    row_index,
                                    column_index + text.column_count(),
                                );
                                let end_y = origin_y + line.scale();
                                if (start_y..=end_y).contains(&position.y)
                                    && (start_x..=end_x).contains(&position.x)
                                {
                                    return Some((
                                        Position {
                                            line_index,
                                            byte_index,
                                        },
                                        Affinity::Before,
                                    ));
                                }
                                column_index += text.column_count();
                            }
                            WrappedElement::Widget(widget) => {
                                column_index += widget.column_count;
                            }
                            WrappedElement::Wrap => {
                                let (_, y) =
                                    line.grid_to_normalized_position(row_index, column_index);
                                let start_y = origin_y + y;
                                let end_y = start_y + line.scale();
                                if (start_y..=end_y).contains(&position.y) {
                                    return Some((
                                        Position {
                                            line_index,
                                            byte_index,
                                        },
                                        Affinity::Before,
                                    ));
                                }
                                column_index = line.wrap_indent_column_count();
                                row_index += 1;
                            }
                        }
                    }
                    let (_, y) = line.grid_to_normalized_position(row_index, column_index);
                    let start_y = origin_y + y;
                    let end_y = start_y + line.scale();
                    if (start_y..=end_y).contains(&position.y) {
                        return Some((
                            Position {
                                line_index,
                                byte_index,
                            },
                            Affinity::After,
                        ));
                    }
                    line_index += 1;
                    origin_y += line.height();
                }
                BlockElement::Line {
                    is_inlay: true,
                    line,
                } => {
                    let start_y = origin_y;
                    let end_y = start_y + line.height();
                    if (start_y..=end_y).contains(&position.y) {
                        return Some((
                            Position {
                                line_index,
                                byte_index: 0,
                            },
                            Affinity::Before,
                        ));
                    }
                    origin_y += line.height();
                }
                BlockElement::Widget(widget) => {
                    origin_y += widget.height;
                }
            }
        }
        None
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CodeEditorAction {
    TextDidChange,
}

struct DrawSelectionLayer<'a> {
    code_editor: &'a mut CodeEditor,
    active_selection: Option<ActiveSelection>,
    selections: Iter<'a, Selection>,
}

impl<'a> DrawSelectionLayer<'a> {
    fn draw_selection_layer(&mut self, cx: &mut Cx2d, session: &Session) {
        let mut line_index = self.code_editor.line_start;
        let mut origin_y = session.layout().line(line_index).y();
        for block in session
            .layout()
            .block_elements(self.code_editor.line_start, self.code_editor.line_end)
        {
            match block {
                BlockElement::Line {
                    is_inlay: false,
                    line,
                } => {
                    let mut byte_index = 0;
                    let mut row_index = 0;
                    let mut column_index = 0;
                    self.handle_event(
                        cx,
                        line_index,
                        line,
                        byte_index,
                        Affinity::Before,
                        origin_y,
                        row_index,
                        column_index,
                    );
                    for element in line.wrapped_elements() {
                        match element {
                            WrappedElement::Text {
                                is_inlay: false,
                                text,
                            } => {
                                for grapheme in text.graphemes() {
                                    self.handle_event(
                                        cx,
                                        line_index,
                                        line,
                                        byte_index,
                                        Affinity::After,
                                        origin_y,
                                        row_index,
                                        column_index,
                                    );
                                    byte_index += grapheme.len();
                                    column_index += grapheme.column_count();
                                    self.handle_event(
                                        cx,
                                        line_index,
                                        line,
                                        byte_index,
                                        Affinity::Before,
                                        origin_y,
                                        row_index,
                                        column_index,
                                    );
                                }
                            }
                            WrappedElement::Text {
                                is_inlay: true,
                                text,
                            } => {
                                column_index += text.column_count();
                            }
                            WrappedElement::Widget(widget) => {
                                column_index += widget.column_count;
                            }
                            WrappedElement::Wrap => {
                                if self.active_selection.is_some() {
                                    self.draw_selection(
                                        cx,
                                        line,
                                        origin_y,
                                        row_index,
                                        column_index,
                                    );
                                }
                                column_index = line.wrap_indent_column_count();
                                row_index += 1;
                            }
                        }
                    }
                    self.handle_event(
                        cx,
                        line_index,
                        line,
                        byte_index,
                        Affinity::After,
                        origin_y,
                        row_index,
                        column_index,
                    );
                    column_index += 1;
                    if self.active_selection.is_some() {
                        self.draw_selection(cx, line, origin_y, row_index, column_index);
                    }
                    line_index += 1;
                    origin_y += line.height();
                }
                BlockElement::Line {
                    is_inlay: true,
                    line,
                } => {
                    origin_y += line.height();
                }
                BlockElement::Widget(widget) => {
                    origin_y += widget.height;
                }
            }
        }
        if self.active_selection.is_some() {
            self.code_editor.draw_selection.end(cx);
        }
    }

    fn handle_event(
        &mut self,
        cx: &mut Cx2d,
        line_index: usize,
        line: Line<'_>,
        byte_index: usize,
        affinity: Affinity,
        origin_y: f64,
        row_index: usize,
        column_index: usize,
    ) {
        let position = Position {
            line_index,
            byte_index,
        };
        if self.active_selection.as_ref().map_or(false, |selection| {
            selection.selection.end() == position && selection.selection.end_affinity() == affinity
        }) {
            self.draw_selection(cx, line, origin_y, row_index, column_index);
            self.code_editor.draw_selection.end(cx);
            let selection = self.active_selection.take().unwrap().selection;
            if selection.cursor.position == position && selection.cursor.affinity == affinity {
                self.draw_cursor(cx, line, origin_y, row_index, column_index);
            }
        }
        if self
            .selections
            .as_slice()
            .first()
            .map_or(false, |selection| {
                selection.start() == position && selection.start_affinity() == affinity
            })
        {
            let selection = *self.selections.next().unwrap();
            if selection.cursor.position == position && selection.cursor.affinity == affinity {
                self.draw_cursor(cx, line, origin_y, row_index, column_index);
            }
            if !selection.is_empty() {
                let (start_x, _) = line.grid_to_normalized_position(row_index, column_index);
                self.active_selection = Some(ActiveSelection { selection, start_x });
            }
            self.code_editor.draw_selection.begin();
        }
    }

    fn draw_selection(
        &mut self,
        cx: &mut Cx2d,
        line: Line<'_>,
        origin_y: f64,
        row_index: usize,
        column_index: usize,
    ) {
        let start_x = mem::take(&mut self.active_selection.as_mut().unwrap().start_x);
        let (x, y) = line.grid_to_normalized_position(row_index, column_index);
        self.code_editor.draw_selection.draw(
            cx,
            Rect {
                pos: DVec2 {
                    x: start_x,
                    y: origin_y + y,
                } * self.code_editor.cell_size
                    + self.code_editor.viewport_rect.pos,
                size: DVec2 {
                    x: x - start_x,
                    y: line.scale(),
                } * self.code_editor.cell_size,
            },
        );
    }

    fn draw_cursor(
        &mut self,
        cx: &mut Cx2d<'_>,
        line: Line<'_>,
        origin_y: f64,
        row_index: usize,
        column_index: usize,
    ) {
        let (x, y) = line.grid_to_normalized_position(row_index, column_index);
        self.code_editor.draw_cursor.draw_abs(
            cx,
            Rect {
                pos: DVec2 { x, y: origin_y + y } * self.code_editor.cell_size
                    + self.code_editor.viewport_rect.pos,
                size: DVec2 {
                    x: 2.0,
                    y: line.scale() * self.code_editor.cell_size.y,
                },
            },
        );
    }
}

struct ActiveSelection {
    selection: Selection,
    start_x: f64,
}

#[derive(Live, LiveHook)]
struct TokenColors {
    #[live]
    unknown: Vec4,
    #[live]
    branch_keyword: Vec4,
    #[live]
    comment: Vec4,
    #[live]
    constant: Vec4,
    #[live]
    identifier: Vec4,
    #[live]
    loop_keyword: Vec4,
    #[live]
    number: Vec4,
    #[live]
    other_keyword: Vec4,
    #[live]
    punctuator: Vec4,
    #[live]
    string: Vec4,
    #[live]
    typename: Vec4,
    #[live]
    whitespace: Vec4,
}

#[derive(Live, LiveHook)]
#[repr(C)]
pub struct DrawIndentGuide {
    #[deref]
    draw_super: DrawQuad,
    #[live]
    color: Vec4,
}

#[derive(Live, LiveHook)]
#[repr(C)]
struct DrawSelection {
    #[deref]
    draw_super: DrawQuad,
    #[live]
    prev_x: f32,
    #[live]
    prev_w: f32,
    #[live]
    next_x: f32,
    #[live]
    next_w: f32,
    #[rust]
    prev_prev_rect: Option<Rect>,
    #[rust]
    prev_rect: Option<Rect>,
}

impl DrawSelection {
    fn begin(&mut self) {
        debug_assert!(self.prev_rect.is_none());
    }

    fn end(&mut self, cx: &mut Cx2d) {
        self.draw_rect_internal(cx, None);
        self.prev_prev_rect = None;
        self.prev_rect = None;
    }

    fn draw(&mut self, cx: &mut Cx2d, rect: Rect) {
        self.draw_rect_internal(cx, Some(rect));
        self.prev_prev_rect = self.prev_rect;
        self.prev_rect = Some(rect);
    }

    fn draw_rect_internal(&mut self, cx: &mut Cx2d, rect: Option<Rect>) {
        if let Some(prev_rect) = self.prev_rect {
            if let Some(prev_prev_rect) = self.prev_prev_rect {
                self.prev_x = prev_prev_rect.pos.x as f32;
                self.prev_w = prev_prev_rect.size.x as f32;
            } else {
                self.prev_x = 0.0;
                self.prev_w = 0.0;
            }
            if let Some(rect) = rect {
                self.next_x = rect.pos.x as f32;
                self.next_w = rect.size.x as f32;
            } else {
                self.next_x = 0.0;
                self.next_w = 0.0;
            }
            self.draw_abs(cx, prev_rect);
        }
    }
}