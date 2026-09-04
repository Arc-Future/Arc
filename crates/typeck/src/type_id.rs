pub use ast::TypeId;

#[derive(Clone, Debug)]
pub enum LinqPath {
    Enumerable,
    Queryable,
}
