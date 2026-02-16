pub(crate) fn framework_label(framework: Option<&str>) -> String {
    framework
        .map(str::to_string)
        .unwrap_or_else(|| "unknown".to_string())
}
