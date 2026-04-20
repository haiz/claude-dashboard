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

    private let service: UpdateService
    private var backgroundTimer: Timer?
    private let rateLimitKey = "claude-dashboard.lastUpdateCheckAt"
    private let rateLimitInterval: TimeInterval = 15 * 60

    init(service: UpdateService = UpdateService()) {
        self.service = service
    }

    func startBackgroundChecks() {
        guard backgroundTimer == nil else { return }
        Task { await checkNow(autoInstall: true, respectRateLimit: true) }
        backgroundTimer = Timer.scheduledTimer(withTimeInterval: 6 * 3600, repeats: true) { [weak self] _ in
            Task { [weak self] in await self?.checkNow(autoInstall: true, respectRateLimit: false) }
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

    private func downloadAndInstall(_ info: UpdateInfo) async {
        state = .downloading
        do {
            let zipURL = try await service.download(info)
            state = .installing
            try service.installAndRelaunch(zipURL: zipURL)
        } catch {
            state = .error(error.localizedDescription)
        }
    }
}
