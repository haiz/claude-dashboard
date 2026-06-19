class ClaudeDashboardCli < Formula
  desc "Terminal dashboard for Claude.ai token usage"
  homepage "https://github.com/haiz/claude-dashboard"
  url "https://github.com/haiz/claude-dashboard/releases/download/v1.14.3/claude-dashboard-cli.tar.gz"
  sha256 "124764e55e2f522d133f6308f05cac19274f8060342af504cc38d13b346180cb"
  version "1.14.3"

  depends_on "jq"
  depends_on :macos => :ventura

  def install
    bin.install "claude-dashboard-helper"
    bin.install "claude-dashboard-cli"
  end

  test do
    assert_match "claude-dashboard-helper", shell_output("#{bin}/claude-dashboard-helper 2>&1", 1)
    assert_match "Usage", shell_output("#{bin}/claude-dashboard-cli --help")
  end
end
