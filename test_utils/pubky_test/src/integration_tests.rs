use pubky_test_macro::pubky_testcase;

#[tokio::test]
#[pubky_testcase]
async fn simple_async_test() {
    let result = 2 + 2;
    assert_eq!(result, 4);
}

#[tokio::test]
#[pubky_testcase]
async fn simple_async_test_that_panics() {
    panic!("This test intentionally panics");
}

#[tokio::test]
#[pubky_testcase]
async fn async_test_with_sleep() {
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    let result = 2 + 2;
    assert_eq!(result, 4);
}

#[tokio::test]
#[pubky_testcase]
async fn async_test_with_sleep_that_panics() {
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    panic!("This async test intentionally panics");
}

#[test]
#[pubky_testcase]
fn simple_sync_test() {
    let result = 2 + 2;
    assert_eq!(result, 4);
}

#[test]
#[pubky_testcase]
fn sync_test_that_panics() {
    panic!("This sync test intentionally panics");
}
