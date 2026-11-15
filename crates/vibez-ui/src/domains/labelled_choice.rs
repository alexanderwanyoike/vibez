//! One entry in a picker: the value it chooses and the words it is shown with.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelledChoice<T> {
    pub value: T,
    pub label: String,
}

impl<T> std::fmt::Display for LabelledChoice<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.label)
    }
}
