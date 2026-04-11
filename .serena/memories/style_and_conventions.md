# Code Style & Conventions

## Naming
- Swift standard naming: camelCase for properties/methods, PascalCase for types
- Services as enums (ChromeCookieService, KeychainService) for namespace-only types with static methods
- Services as classes (UsageAPIService, AccountStore) when instances with state are needed

## Patterns
- **MVVM architecture** — Models, ViewModels, Views clearly separated
- **@MainActor** on ViewModel for thread safety
- **Combine** for reactive state (@Published properties)
- **TaskGroup** for parallel async operations
- **Enum-based services** for stateless utility collections
- **Namespace comments** (e.g. `// MARK: - Profile Scanning`) to organize methods within types

## SwiftUI Conventions
- Views are structs conforming to View protocol
- Body computed property for view hierarchy
- Extracted sub-views as computed properties (e.g. `emptyState`, `accountList`)

## Testing
- MockURLProtocol for network mocking
- Isolated UserDefaults suites for test isolation
- Test files mirror source structure: `ClaudeDashboardTests/`

## API Patterns
- ISO8601 date parsing handles both with and without fractional seconds (.SSS)
- Session key refresh handled via Set-Cookie response headers
- Plan tier detection from `extra_usage` API response field
