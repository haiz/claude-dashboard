run:
	@xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -1 && open ~/Library/Developer/Xcode/DerivedData/ClaudeDashboard-*/Build/Products/Debug/ClaudeDashboard.app

cli-build:
	@xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboardHelper -configuration Release build 2>&1 | tail -1

cli-run: cli-build
	@BINARY=$$(find ~/Library/Developer/Xcode/DerivedData/ClaudeDashboard-*/Build/Products/Release -name claude-dashboard-helper -type f | head -1) && \
	ln -sf "$$BINARY" cli/claude-dashboard-helper && \
	cli/claude-dashboard-cli

cli-once: cli-build
	@BINARY=$$(find ~/Library/Developer/Xcode/DerivedData/ClaudeDashboard-*/Build/Products/Release -name claude-dashboard-helper -type f | head -1) && \
	ln -sf "$$BINARY" cli/claude-dashboard-helper && \
	cli/claude-dashboard-cli --once
