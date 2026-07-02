use codedb_fixture_demo_macro::demo_attr;

#[demo_attr]
pub fn decorated() -> &'static str {
    "decorated"
}
