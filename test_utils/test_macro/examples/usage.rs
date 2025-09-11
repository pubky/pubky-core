use pubky_test_utils_macro::pubky_testcase;

// Example of using the macro with an async test
#[tokio::test]
#[pubky_testcase]
async fn simple_async_test() {
    // Your test logic here
    assert_eq!(2 + 2, 4);
    println!("Inside the test: doing some work");
}

// Example of using the macro with an async test that has sleep
#[tokio::test]
#[pubky_testcase]
async fn async_test_with_sleep() {
    // Your async test logic here
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    assert_eq!(3 + 3, 6);
    println!("Inside the async test: doing some async work");
}

// Example of an async test that panics (to show cleanup still executes)
#[tokio::test]
#[pubky_testcase]
async fn async_test_that_panics() {
    println!("This test will panic, but cleanup will still execute");
    panic!("Intentional panic for demonstration");
}

// You can also use other test attributes like #[ignore] or custom ones
#[tokio::test]
#[ignore]
#[pubky_testcase]
async fn ignored_async_test() {
    assert_eq!(1 + 1, 2);
    println!("This test is ignored but would still print if run");
}

fn main() {
    println!("This is an example of how to use the test macros.");
    println!("Run 'cargo test' to see the macros in action.");
    println!("The macros will wrap your tests and execute println statements at the end.");
}
