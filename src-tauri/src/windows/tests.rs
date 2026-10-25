use super::*;

#[test]
fn start_urls_carry_the_page_in_the_fragment() {
    assert_eq!(start_url(None, None, None), "index.html");
    assert_eq!(start_url(Some("reports"), None, None), "index.html#view=reports");
    let data = serde_json::json!({ "accountId": 7, "startDate": "2026-01-01" });
    let url = start_url(Some("accounting/register"), Some("open"), Some(&data));
    assert_eq!(url, "index.html#view=accounting%2Fregister&action=open&data=%7B%22accountId%22%3A7%2C%22startDate%22%3A%222026-01-01%22%7D");
    assert_eq!(start_url(Some("reports"), None, Some(&serde_json::Value::Null)), "index.html#view=reports");
}
