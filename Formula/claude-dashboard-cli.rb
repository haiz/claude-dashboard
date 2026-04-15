class ClaudeDashboardCli < Formula
  desc "Terminal dashboard for Claude.ai token usage"
  homepage "https://github.com/haiz/claude-dashboard"
  url "https://github.com/haiz/claude-dashboard/releases/download/v1.4.0/claude-dashboard-cli.tar.gz"
  sha256 "df86d44873c515a26c9bf5bc511c188c4b24dfc725537950ba77a3db3ac175a1"
  version "1.4.0"

  depends_on "jq"
  depends_on :macos => :ventura

  def install
    cd "claude-dashboard-cli-v#{version}"
    bin.install "claude-dashboard-helper"
    bin.install "claude-dashboard-cli"
  end

  test do
    assert_match "claude-dashboard-helper", shell_output("#{bin}/claude-dashboard-helper 2>&1", 1)
    assert_match "Usage", shell_output("#{bin}/claude-dashboard-cli --help")
  end
end
