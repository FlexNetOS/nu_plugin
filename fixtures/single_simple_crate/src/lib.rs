pub struct Greeting {
    pub name: String,
}

pub fn greet(input: &Greeting) -> String {
    format!("hello {}", input.name)
}
