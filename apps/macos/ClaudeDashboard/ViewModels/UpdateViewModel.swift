import Foundation
import Combine

@MainActor
final class UpdateViewModel: ObservableObject {
    enum State: Equatable {
        case idle
        case checking
        case upToDate
        case available(UpdateInfo)
        case downloading
        case installing
        case error(String)

        var statusLabel: String {
            switch self {
            case .idle: return ""
            case .checking: return "Checking for updates…"
            case .upToDate: return "Up to date"
            case .available(let info): return "v\(info.version) available"
            case .downloading: return "Downloading update…"
            case .installing: return "Installing update…"
            case .error(let msg): return msg
            }
        }

        var isWorking: Bool {
            switch self {
            case .checking, .downloading, .installing: return true
            default: return false
            }
        }
    }

    @Published var state: State = .idle
    @Published var latestVersion: String?
    @Published var autoUpdateEnabled: Bool {
        didSet {
            UserDefaults.standard.set(autoUpdateEnabled, forKey: autoUpdateEnabledKey)
            restartBackgroundChecks()
        }
    }

    private let service: UpdateService
    private var backgroundTimer: Timer?
    private let rateLimitKey = "claude-dashboard.lastUpdateCheckAt"
    private let rateLimitInterval: TimeInterval = 15 * 60
    private let autoUpdateEnabledKey = "claude-dashboard.autoUpdateEnabled"
    private let autoUpdateIntervalKey = "claude-dashboard.lastAutoUpdateAt"
    private let autoUpdateInterval: TimeInterval = 24 * 3600

    init(service: UpdateService = UpdateService()) {
        self.service = service
        self.autoUpdateEnabled = UserDefaults.standard.object(forKey: "claude-dashboard.autoUpdateEnabled") as? Bool ?? true
    }

    func startBackgroundChecks() {
        guard backgroundTimer == nil else { return }
        guard autoUpdateEnabled else { return }
        guard ProcessInfo.processInfo.environment["DISABLE_AUTOUPDATER"] != "1" else { return }

        // Check immediately on launch if 24h have elapsed since last auto-update
        Task { await checkNowIfDue() }

        // Re-check every hour; actual install only runs when 24h gate passes
        backgroundTimer = Timer.scheduledTimer(withTimeInterval: 3600, repeats: true) { [weak self] _ in
            Task { [weak self] in await self?.checkNowIfDue() }
        }
    }

    func checkNow(autoInstall: Bool = false, respectRateLimit: Bool = false) async {
        if respectRateLimit, let last = UserDefaults.standard.object(forKey: rateLimitKey) as? Date,
           Date().timeIntervalSince(last) < rateLimitInterval {
            return
        }

        state = .checking
        UserDefaults.standard.set(Date(), forKey: rateLimitKey)

        do {
            let info = try await service.checkForUpdate()
            if let info {
                latestVersion = info.version
                if autoInstall {
                    await downloadAndInstall(info)
                } else {
                    state = .available(info)
                }
            } else {
                state = .upToDate
            }
        } catch {
            state = .error(error.localizedDescription)
        }
    }

    private func checkNowIfDue() async {
        let last = UserDefaults.standard.object(forKey: autoUpdateIntervalKey) as? Date
        guard last == nil || Date().timeIntervalSince(last!) >= autoUpdateInterval else { return }
        UserDefaults.standard.set(Date(), forKey: autoUpdateIntervalKey)
        await checkNow(autoInstall: true, respectRateLimit: false)
    }

    private func restartBackgroundChecks() {
        backgroundTimer?.invalidate()
        backgroundTimer = nil
        startBackgroundChecks()
    }

    private func downloadAndInstall(_ info: UpdateInfo) async {
        state = .downloading
        do {
            let zipURL = try await service.download(info)
            state = .installing
            try await service.installAndRelaunch(zipURL: zipURL)
            exit(0)
        } catch {
            state = .error(error.localizedDescription)
        }
    }
}
