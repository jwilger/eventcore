use super::*;

#[test]
fn event_page_first_has_no_after_position() {
    let page = EventPage::first(BatchSize::new(100));
    assert_eq!(page.after_position(), None);
    let limit: usize = page.limit().into();
    assert_eq!(limit, 100);
}

#[test]
fn event_page_after_has_correct_position() {
    let uuid = Uuid::parse_str("018e8c5e-8c5e-7000-8000-000000000001").unwrap();
    let position = StreamPosition::new(uuid);
    let page = EventPage::after(position, BatchSize::new(50));
    assert_eq!(page.after_position(), Some(position));
    let limit: usize = page.limit().into();
    assert_eq!(limit, 50);
}

#[test]
fn event_page_next_preserves_limit_and_updates_position() {
    let page = EventPage::first(BatchSize::new(100));
    let uuid = Uuid::parse_str("018e8c5e-8c5e-7000-8000-000000000002").unwrap();
    let new_position = StreamPosition::new(uuid);
    let next_page = page.next(new_position);
    assert_eq!(next_page.after_position(), Some(new_position));
    let limit: usize = next_page.limit().into();
    assert_eq!(limit, 100);
}
