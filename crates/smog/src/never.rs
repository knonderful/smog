/// A marker type to specify a function that never returns. Since this type is not constructible
/// (the constructor is private inside of this module) it would be impossible to write a function
/// that returns an instance of this. Therefor the function must be infinite.
pub struct Never(());
