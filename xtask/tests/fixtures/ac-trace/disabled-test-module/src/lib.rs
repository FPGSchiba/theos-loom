//! The only test of the second criterion sits in a module that no build compiles.

#[cfg(test)]
mod tests {
    // AC: CE-90.1
    #[test]
    fn proves_the_first() {}
}

#[cfg(any())]
mod never_compiled {
    // AC: CE-90.2
    #[test]
    fn would_prove_the_second() {}
}
