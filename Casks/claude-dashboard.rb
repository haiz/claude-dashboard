cask "claude-dashboard" do
  version "1.1.0"
  sha256 "9fac2c6c84a6693aef7a31f306e0c893058cccf9e4d7eea4e2e17745be7a6202"

  url "https://github.com/haiz/claude-dashboard/releases/download/v#{version}/ClaudeDashboard.app.zip"
  name "Claude Dashboard"
  desc "macOS menu bar app for monitoring Claude.ai token usage"
  homepage "https://github.com/haiz/claude-dashboard"

  depends_on macos: ">= :ventura"

  app "ClaudeDashboard.app"

  zap trash: [
    "~/Library/Preferences/com.claude-dashboard.app.plist",
    "~/Library/Application Support/ClaudeDashboard",
  ]
end
