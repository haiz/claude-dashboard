cask "claude-dashboard" do
  version "1.1.0"
  sha256 "5aaaab4570b5cd71a227c20cdf6093617741dfe08d77ffad4f7745c958f3fa82"

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
