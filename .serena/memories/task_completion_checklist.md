# Task Completion Checklist

When a coding task is completed, perform these steps:

1. **Regenerate project if needed** — If new files or targets were added, run `xcodegen generate`
2. **Build** — `xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build`
3. **Run tests** — `xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests test`
4. **Verify no regressions** — Ensure all existing tests still pass
5. **Check consistency** — Ensure changes follow MVVM pattern and existing code style
