import SwiftUI

struct DetectedAccount: Identifiable {
    var id: String { "\(browser.rawValue):\(chromeProfilePath)" }
    let browser: Browser
    let orgId: String
    let accountUuid: String
    let chromeProfilePath: String
    let chromeProfileName: String
    let chromeProfileGoogleEmail: String
    let sessionKey: String
    var accountName: String
    var email: String?
    var plan: AccountPlan?
    var isSelected: Bool = true
}

struct SetupView: View {
    @ObservedObject var viewModel: DashboardViewModel
    var onDone: (() -> Void)?
    @Environment(\.dismiss) private var dismiss

    @State private var detectedAccounts: [DetectedAccount] = []
    @State private var isScanning = false
    @State private var scanError: String?
    @State private var selectedBrowser: Browser = .chrome
    @State private var installedBrowsers: [Browser] = []
    @State private var scanTask: Task<Void, Never>?
    /// Khi true: máy có 2+ browser và chưa có preference đã lưu, buộc người dùng
    /// chọn browser trước. Chưa scan (nên prompt Keychain chưa xuất hiện).
    @State private var awaitingBrowserChoice = false

    private static let preferredBrowserKey = "preferredScanBrowser"

    var body: some View {
        VStack(spacing: 16) {
            Text("Setup — Sync from Browser")
                .font(.title2.bold())

            if awaitingBrowserChoice {
                browserChooserView
            } else {
                if !installedBrowsers.isEmpty {
                    Picker("Browser", selection: $selectedBrowser) {
                        ForEach(installedBrowsers, id: \.self) { browser in
                            Text(browser.displayName).tag(browser)
                        }
                    }
                    .pickerStyle(.menu)
                    .frame(maxWidth: 240)
                    .onChange(of: selectedBrowser) { newValue in
                        UserDefaults.standard.set(newValue.rawValue, forKey: Self.preferredBrowserKey)
                        scan()
                    }
                }

                if isScanning {
                    VStack(spacing: 8) {
                        ProgressView()
                        Text("Scanning \(selectedBrowser.displayName) profiles and detecting accounts...")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                } else if detectedAccounts.isEmpty {
                    noProfilesView
                } else {
                    accountList
                }
            }

            HStack {
                Button("Cancel") {
                    dismissSelf()
                }
                .keyboardShortcut(.cancelAction)

                Spacer()

                if !detectedAccounts.isEmpty {
                    Button("Add Selected") {
                        Task {
                            await addSelectedAccounts()
                            dismissSelf()
                        }
                    }
                    .keyboardShortcut(.defaultAction)
                    .disabled(detectedAccounts.filter(\.isSelected).isEmpty)
                }
            }
        }
        .padding(24)
        .frame(width: 520, height: 450)
        .onAppear {
            installedBrowsers = BrowserCookieService.installedBrowsers()

            if let saved = savedPreferredBrowser() {
                // Đã có browser đã nhớ và vẫn cài: quét luôn như trước.
                selectedBrowser = saved
                scan()
            } else if installedBrowsers.count == 1, let only = installedBrowsers.first {
                // Chỉ một browser: không cần hỏi, quét luôn.
                selectedBrowser = only
                scan()
            } else if installedBrowsers.count >= 2 {
                // Nhiều browser và chưa có preference: buộc người dùng chọn trước.
                // Chưa scan() để prompt truy cập cookies chỉ hiện sau khi đã chọn.
                awaitingBrowserChoice = true
            } else {
                // Không có browser được hỗ trợ: scan() để hiển thị thông báo phù hợp.
                scan()
            }
        }
    }

    private func dismissSelf() {
        onDone?()
        dismiss()
    }

    /// Browser đã nhớ từ lần trước, nếu hợp lệ và vẫn còn cài. nil nếu chưa từng chọn.
    private func savedPreferredBrowser() -> Browser? {
        guard let raw = UserDefaults.standard.string(forKey: Self.preferredBrowserKey),
              let saved = Browser(rawValue: raw),
              installedBrowsers.contains(saved) else { return nil }
        return saved
    }

    /// Người dùng chọn browser từ màn chooser: nhớ lựa chọn rồi bắt đầu quét.
    private func chooseBrowser(_ browser: Browser) {
        UserDefaults.standard.set(browser.rawValue, forKey: Self.preferredBrowserKey)
        awaitingBrowserChoice = false
        // Gán selectedBrowser có thể kích hoạt Picker.onChange gọi scan() thêm một lần;
        // scanTask?.cancel() trong scan() đã xử lý trường hợp trùng này.
        selectedBrowser = browser
        scan()
    }

    private var browserChooserView: some View {
        VStack(spacing: 12) {
            Text("Choose the browser where you're signed in to Claude")
                .font(.headline)
                .multilineTextAlignment(.center)

            Text("We'll only read cookies from the browser you pick.")
                .font(.caption)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)

            VStack(spacing: 8) {
                ForEach(installedBrowsers, id: \.self) { browser in
                    Button {
                        chooseBrowser(browser)
                    } label: {
                        HStack(spacing: 10) {
                            Image(systemName: "globe")
                                .foregroundStyle(.secondary)
                            Text(browser.displayName)
                                .font(.body)
                            Spacer()
                            Image(systemName: "chevron.right")
                                .font(.caption)
                                .foregroundStyle(.tertiary)
                        }
                        .padding(.vertical, 10)
                        .padding(.horizontal, 14)
                        .frame(maxWidth: .infinity)
                        .background(Color.primary.opacity(0.05))
                        .clipShape(RoundedRectangle(cornerRadius: 8))
                    }
                    .buttonStyle(.plain)
                }
            }
            .frame(maxWidth: 320)
            .padding(.top, 4)
        }
        .frame(maxWidth: .infinity)
    }

    private var noProfilesView: some View {
        VStack(spacing: 8) {
            Image(systemName: "exclamationmark.triangle")
                .font(.largeTitle)
                .foregroundStyle(.orange)

            Text(scanError ?? (installedBrowsers.isEmpty
                ? "No supported browser found (Chrome, Arc, Brave, Edge)."
                : "No \(selectedBrowser.displayName) profiles found with active Claude sessions."))
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)

            Button("Retry Scan") { scan() }
                .padding(.top, 8)
        }
    }

    private var accountList: some View {
        List {
            ForEach($detectedAccounts) { $account in
                HStack {
                    Toggle(isOn: $account.isSelected) {
                        VStack(alignment: .leading, spacing: 2) {
                            HStack(spacing: 6) {
                                Text(account.email ?? account.accountName)
                                    .font(.body.bold())
                                if let plan = account.plan {
                                    Text(plan.rawValue)
                                        .font(.caption2)
                                        .padding(.horizontal, 6)
                                        .padding(.vertical, 2)
                                        .background(plan.badgeColor.opacity(0.15))
                                        .clipShape(Capsule())
                                }
                            }
                            let browserLabel = account.browser.displayName
                            let chromeEmail = account.chromeProfileGoogleEmail
                            Text("\(browserLabel): \(chromeEmail.isEmpty ? account.chromeProfilePath : chromeEmail)")
                                .font(.caption)
                                .foregroundStyle(.tertiary)
                        }
                    }
                }
                .padding(.vertical, 4)
            }
        }
    }

    private func scan() {
        // Hủy scan đang chạy (vd user đổi picker, hoặc onAppear gán browser != .chrome
        // làm onChange kích hoạt thêm một lần) — tránh hai Task ghi đè state lẫn nhau.
        scanTask?.cancel()
        isScanning = true
        scanError = nil

        scanTask = Task {
            let browser = selectedBrowser
            let results = await Task.detached {
                BrowserCookieService.profilesWithClaudeSessions(browser: browser)
            }.value

            if Task.isCancelled { return }

            if results.isEmpty {
                await MainActor.run {
                    self.detectedAccounts = []
                    self.isScanning = false
                    self.scanError = "No \(selectedBrowser.displayName) profiles found with active Claude sessions. Make sure you're logged into claude.ai in \(selectedBrowser.displayName)."
                }
                return
            }

            let apiService = UsageAPIService()
            var accounts: [DetectedAccount] = []
            var duplicateCount = 0
            var validationFailureCount = 0

            for item in results {
                guard let sessionKey = item.cookies.sessionKey else {
                    validationFailureCount += 1
                    continue
                }

                // Identity comes from /api/account: the account's own uuid and
                // email, not guessed from an org name. A failure here is the
                // same outcome as an expired session.
                guard let info = try? await apiService.fetchAccount(sessionKey: sessionKey) else {
                    validationFailureCount += 1
                    continue
                }

                let email = info.email

                // The org to poll usage from. Never an identity — colleagues
                // share one. See contract/cases/org-selection.json.
                guard let orgId = AccountIdentity.resolveOrgId(
                    lastActiveOrg: item.cookies.orgId, memberships: info.memberships) else {
                    validationFailureCount += 1
                    continue
                }

                let accountName = email ?? item.profile.displayName

                // Account identity = the Claude account's own uuid. Two members
                // of one organisation are two accounts; two Claude accounts in
                // one browser profile are also two accounts.
                // See contract/cases/dedupe.json.
                let alreadyStored = AccountIdentity.isDuplicate(
                    candidateUuid: info.uuid,
                    candidateEmail: email,
                    against: viewModel.accountStore.accounts.map(StoredIdentity.init)
                )
                if alreadyStored {
                    duplicateCount += 1
                    continue
                }

                // Plan tier from org capabilities (the usage endpoint has no
                // reliable plan signal).
                let plan = (try? await apiService.fetchOrganizations(sessionKey: sessionKey))?
                    .first(where: { $0.uuid == orgId })?.planHint

                accounts.append(DetectedAccount(
                    browser: item.profile.browser,
                    orgId: orgId,
                    accountUuid: info.uuid,
                    chromeProfilePath: item.profile.path,
                    chromeProfileName: item.profile.displayName,
                    chromeProfileGoogleEmail: item.profile.googleEmail,
                    sessionKey: sessionKey,
                    accountName: accountName,
                    email: email,
                    plan: plan,
                    isSelected: true
                ))
            }

            if Task.isCancelled { return }

            await MainActor.run {
                self.detectedAccounts = accounts
                self.isScanning = false
                if accounts.isEmpty && !results.isEmpty {
                    if validationFailureCount > 0 && duplicateCount == 0 {
                        self.scanError = "Couldn't validate sessions for the detected \(selectedBrowser.displayName) profiles. Make sure you're signed in to claude.ai and try again."
                    } else if validationFailureCount > 0 {
                        self.scanError = "Some accounts are already added; couldn't validate the rest. Make sure you're signed in to claude.ai and try again."
                    } else {
                        self.scanError = "All detected accounts are already added."
                    }
                }
            }
        }
    }

    private func addSelectedAccounts() async {
        for detected in detectedAccounts where detected.isSelected {
            // Belt-and-suspenders: scan() already skips duplicates, but in case
            // the store changed between scan and add (or for safety), re-check here.
            let dup = AccountIdentity.isDuplicate(
                candidateUuid: detected.accountUuid,
                candidateEmail: detected.email,
                against: viewModel.accountStore.accounts.map(StoredIdentity.init)
            )
            if dup { continue }

            let displayName = detected.email ?? detected.accountName
            let chromeLabel = detected.chromeProfileGoogleEmail.isEmpty ? detected.chromeProfileName : detected.chromeProfileGoogleEmail
            let encryptedSession = CryptoService.encrypt(detected.sessionKey) ?? detected.sessionKey

            let account = Account(
                id: UUID(),
                name: displayName,
                email: detected.email,
                chromeProfilePath: detected.chromeProfilePath,
                chromeProfileName: chromeLabel,
                orgId: detected.orgId,
                accountUuid: detected.accountUuid,
                sessionKey: encryptedSession,
                browser: detected.browser,
                plan: detected.plan ?? .pro,
                lastSynced: Date(),
                status: .active
            )

            viewModel.accountStore.addAccount(account)
        }

        // Auto-refresh after adding
        Task {
            await viewModel.refreshAll()
        }
    }
}
