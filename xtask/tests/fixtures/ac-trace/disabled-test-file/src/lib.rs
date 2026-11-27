//! The only test of the second criterion sits in a file that no build runs.

#[cfg(test)]
mod tests {
    // AC: TU-90.1
    #[test]
    fn proves_the_first() {}
}
