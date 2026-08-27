use ratatui::layout::Rect;
use ratatui::{
    buffer::Buffer,
    style::{Color, Modifier, Style},
};

const CARD_WIDTH: u16 = 12;
const CARD_HEIGHT: u16 = 2;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ObjectCardId(pub u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectCardKind {
    Object(ObjectCardId),
    Add,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectCard {
    pub kind: ObjectCardKind,
    pub title: String,
    pub deletable: bool,
}

impl ObjectCard {
    pub fn object(id: ObjectCardId, title: impl Into<String>, deletable: bool) -> Self {
        Self {
            kind: ObjectCardKind::Object(id),
            title: title.into(),
            deletable,
        }
    }

    pub fn add() -> Self {
        Self {
            kind: ObjectCardKind::Add,
            title: "+".into(),
            deletable: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectStripHit {
    Select(ObjectCardId),
    Delete(ObjectCardId),
    Add,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaidOutObjectCard {
    pub logical_index: usize,
    pub card: ObjectCard,
    pub body_rect: Rect,
    pub delete_rect: Option<Rect>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectStripLayout {
    pub cards: Vec<LaidOutObjectCard>,
    pub logical_len: usize,
}

impl ObjectStripLayout {
    pub fn hit(&self, x: u16, y: u16) -> Option<ObjectStripHit> {
        for card in &self.cards {
            if card.delete_rect.is_some_and(|rect| contains(rect, x, y))
                && let ObjectCardKind::Object(id) = card.card.kind
            {
                return Some(ObjectStripHit::Delete(id));
            }
            if contains(card.body_rect, x, y) {
                return Some(match card.card.kind {
                    ObjectCardKind::Object(id) => ObjectStripHit::Select(id),
                    ObjectCardKind::Add => ObjectStripHit::Add,
                });
            }
        }
        None
    }
}

pub fn layout_object_strip(cards: &[ObjectCard], area: Rect, scroll: usize) -> ObjectStripLayout {
    let logical_len = cards.len();
    if area.width == 0 || area.height < CARD_HEIGHT {
        return ObjectStripLayout {
            cards: Vec::new(),
            logical_len,
        };
    }
    let card_width = CARD_WIDTH.min(area.width);
    let columns = usize::from((area.width / card_width).max(1));
    let rows = usize::from(area.height / CARD_HEIGHT);
    let capacity = columns.saturating_mul(rows);
    let start = scroll.min(logical_len);
    let cards = cards
        .iter()
        .enumerate()
        .skip(start)
        .take(capacity)
        .map(|(logical_index, card)| {
            let visible_index = logical_index - start;
            let column = visible_index % columns;
            let row = visible_index / columns;
            let body_rect = Rect::new(
                area.x + u16::try_from(column).unwrap_or(u16::MAX) * card_width,
                area.y + u16::try_from(row).unwrap_or(u16::MAX) * CARD_HEIGHT,
                card_width,
                CARD_HEIGHT,
            );
            let delete_rect = card.deletable.then(|| {
                Rect::new(
                    body_rect.x + body_rect.width.saturating_sub(2),
                    body_rect.y,
                    body_rect.width.min(2),
                    1,
                )
            });
            LaidOutObjectCard {
                logical_index,
                card: card.clone(),
                body_rect,
                delete_rect,
            }
        })
        .collect();
    ObjectStripLayout { cards, logical_len }
}

fn contains(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

pub fn render_object_strip(
    buffer: &mut Buffer,
    layout: &ObjectStripLayout,
    selected: Option<ObjectCardId>,
) {
    for laid_out in &layout.cards {
        let is_selected = matches!(
            laid_out.card.kind,
            ObjectCardKind::Object(id) if Some(id) == selected
        );
        let style = if is_selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::LightBlue)
        };
        buffer.set_style(laid_out.body_rect, style);
        match laid_out.card.kind {
            ObjectCardKind::Add => {
                buffer.set_string(laid_out.body_rect.x, laid_out.body_rect.y, "+ Add", style);
            }
            ObjectCardKind::Object(_) => {
                let reserved = if laid_out.delete_rect.is_some() { 3 } else { 1 };
                let available = laid_out.body_rect.width.saturating_sub(reserved).into();
                let title = laid_out
                    .card
                    .title
                    .chars()
                    .take(available)
                    .collect::<String>();
                buffer.set_string(laid_out.body_rect.x, laid_out.body_rect.y, title, style);
                if let Some(delete) = laid_out.delete_rect {
                    buffer.set_string(delete.x, delete.y, "×", Style::default().fg(Color::Red));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ObjectCard, ObjectCardId, ObjectStripHit, layout_object_strip, render_object_strip,
    };
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Color;

    #[test]
    fn wrapped_strip_keeps_every_card_and_hits_delete_before_body() {
        let cards = (0..5)
            .map(|id| ObjectCard::object(ObjectCardId(id), format!("k{id}"), true))
            .chain([ObjectCard::add()])
            .collect::<Vec<_>>();

        let layout = layout_object_strip(&cards, Rect::new(10, 4, 24, 6), 0);

        assert_eq!(layout.cards.len(), 6);
        let selected = &layout.cards[2];
        let delete = selected.delete_rect.expect("deletable card has a target");
        assert_eq!(
            layout.hit(delete.x, delete.y),
            Some(ObjectStripHit::Delete(ObjectCardId(2)))
        );
    }

    #[test]
    fn scrolling_changes_visibility_without_dropping_logical_cards() {
        let cards = (0..8)
            .map(|id| ObjectCard::object(ObjectCardId(id), format!("channel {id}"), false))
            .chain([ObjectCard::add()])
            .collect::<Vec<_>>();

        let first = layout_object_strip(&cards, Rect::new(0, 0, 30, 2), 0);
        let scrolled = layout_object_strip(&cards, Rect::new(0, 0, 30, 2), 4);

        assert_eq!(first.logical_len, 9);
        assert_eq!(scrolled.logical_len, 9);
        assert_ne!(
            first.cards[0].logical_index,
            scrolled.cards[0].logical_index
        );
        assert_eq!(
            scrolled.hit(scrolled.cards[0].body_rect.x, scrolled.cards[0].body_rect.y),
            Some(ObjectStripHit::Select(ObjectCardId(4)))
        );
    }

    #[test]
    fn rendered_strip_shows_selection_delete_and_add_targets() {
        let cards = vec![
            ObjectCard::object(ObjectCardId(7), "state", true),
            ObjectCard::add(),
        ];
        let area = Rect::new(0, 0, 24, 2);
        let layout = layout_object_strip(&cards, area, 0);
        let mut buffer = Buffer::empty(area);

        render_object_strip(&mut buffer, &layout, Some(ObjectCardId(7)));

        assert_eq!(buffer.cell((0, 0)).unwrap().fg, Color::Yellow);
        assert!(buffer.cell((10, 0)).unwrap().symbol().contains('×'));
        assert!(buffer.cell((12, 0)).unwrap().symbol().contains('+'));
    }
}
