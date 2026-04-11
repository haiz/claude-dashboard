run:
	@xcodebuild -project ClaudeDashboard.xcodeproj -scheme ClaudeDashboard build 2>&1 | tail -1 && open ~/Library/Developer/Xcode/DerivedData/ClaudeDashboard-*/Build/Products/Debug/ClaudeDashboard.app
