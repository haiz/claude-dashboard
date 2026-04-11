# Claude Dashboard - Project Overview

## Purpose
macOS menu bar app (SwiftUI) that monitors Claude.ai token usage across multiple accounts. Extracts session keys from Chrome's encrypted cookie database, fetches usage data from Claude.ai API, and displays real-time utilization with burn-rate-based sorting.

## Tech Stack
- **Language:** Swift 5.0
- **UI Framework:** SwiftUI + AppKit (MenuBarExtra)
- **Target:** macOS 13.0+
- **Build System:** XcodeGen (project.yml → .xcodeproj)
- **Dependencies:** None — pure native Swift (SwiftUI, AppKit, Combine, Security, CommonCrypto, SQLite3)

## Key Characteristics
- LSUIElement: true — runs as menu bar only (no Dock icon)
- App Sandbox disabled — required for Chrome cookie DB access and Keychain reads
- No external package dependencies
- Tests use MockURLProtocol for network mocking and isolated UserDefaults suites

## Data Flow
Chrome cookies (SQLite + AES decryption) → Session keys (Keychain) → Claude.ai API → Usage data → ViewModel → SwiftUI views
