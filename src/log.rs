#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {{
        println!($($arg)*);
    }};
}

#[macro_export]
macro_rules! bail {
    ($($arg:tt)*) => {{
        eprintln!("ERROR ERROR ERROR!!!");
        eprintln!($($arg)*);
        std::process::exit(1);
    }};
}
