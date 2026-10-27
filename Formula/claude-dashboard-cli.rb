class ClaudeDashboardCli < Formula
  desc "Terminal dashboard for Claude.ai token usage"
  homepage "https://github.com/haiz/claude-dashboard"
  url "https://github.com/haiz/claude-dashboard/releases/download/v1.34.0/claude-dashboard-cli.tar.gz"
  sha256 "4d75ca70571cf5c60f04401aeafc62cfa956b2be6880b43b4ff0214a2b91d0d3"
  version "1.34.0"

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
