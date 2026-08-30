# Suggested Commands

## Build & Project Generation
```bash
# Generate Xcode project from project.yml (required after adding files/targets)
cd apps/macos && xcodegen generate

# Build the app
xcodebuild -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build
```

## Testing
```bash
# Run all tests
xcodebuild -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests test

# Run a single test class
xcodebuild test -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/UsageDataTests

# Run a single test method
xcodebuild test -project apps/macos/ClaudeDashboard.xcodeproj -scheme ClaudeDashboardTests -only-testing:ClaudeDashboardTests/UsageDataTests/testDecodeUsageData
```

## Test Files
- apps/macos/ClaudeDashboardTests/UsageAPIServiceTests.swift
- apps/macos/ClaudeDashboardTests/UsageDataTests.swift
- apps/macos/ClaudeDashboardTests/ChromeCookieServiceTests.swift
- apps/macos/ClaudeDashboardTests/AccountStoreTests.swift

## System Commands (Darwin/macOS)
- `git` — version control
- `cd apps/macos && xcodegen generate` — regenerate .xcodeproj from project.yml
- `xcodebuild` — build and test

## Notes
- No linting or formatting tools configured
- No external dependencies to install
- Edit `project.yml` for target/build setting changes, then run `cd apps/macos && xcodegen generate`
