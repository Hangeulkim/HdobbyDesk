import UIKit
import Flutter

@main
@objc class AppDelegate: FlutterAppDelegate {
  private let clipboardChannelName = "com.hdobby.hdobbydesk/clipboard"
  private let maxClipboardTextBytes = 4 * 1024 * 1024
  private let maxClipboardPngBytes = 24 * 1024 * 1024
  private let maxClipboardImagePixels: UInt64 = 16_000_000
  private var clipboardChannel: FlutterMethodChannel?

  override func application(
    _ application: UIApplication,
    didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
  ) -> Bool {
    GeneratedPluginRegistrant.register(with: self)
    if let controller = window?.rootViewController as? FlutterViewController {
      let channel = FlutterMethodChannel(
        name: clipboardChannelName,
        binaryMessenger: controller.binaryMessenger
      )
      channel.setMethodCallHandler { [weak self] call, result in
        self?.handleClipboardCall(call, result: result)
      }
      clipboardChannel = channel
    }
    dummyMethodToEnforceBundling();
    return super.application(application, didFinishLaunchingWithOptions: launchOptions)
  }

  private func handleClipboardCall(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
    switch call.method {
    case "readClipboard":
      readClipboard(result: result)
    case "writeClipboard":
      writeClipboard(call.arguments, result: result)
    default:
      result(FlutterMethodNotImplemented)
    }
  }

  private func readClipboard(result: @escaping FlutterResult) {
    let pasteboard = UIPasteboard.general
    var response: [String: Any] = ["changeCount": pasteboard.changeCount]

    if let text = pasteboard.string, !text.isEmpty {
      guard let textData = text.data(using: .utf8), textData.count <= maxClipboardTextBytes else {
        result(FlutterError(code: "Clipboard is too large", message: nil, details: nil))
        return
      }
      response["text"] = text
    }

    var png = pasteboard.data(forPasteboardType: "public.png")
    if png == nil, let image = pasteboard.image {
      png = image.pngData()
    }
    if let png = png {
      guard isValidClipboardPng(png), UIImage(data: png) != nil else {
        result(FlutterError(code: "Clipboard image is invalid", message: nil, details: nil))
        return
      }
      response["png"] = FlutterStandardTypedData(bytes: png)
    }

    if response["text"] == nil && response["png"] == nil {
      result(FlutterError(code: "Clipboard is empty", message: nil, details: nil))
      return
    }
    result(response)
  }

  private func writeClipboard(_ arguments: Any?, result: @escaping FlutterResult) {
    guard let arguments = arguments as? [String: Any] else {
      result(FlutterError(code: "Clipboard format is unsupported", message: nil, details: nil))
      return
    }

    var item: [String: Any] = [:]
    if let text = arguments["text"] as? String, !text.isEmpty {
      guard let textData = text.data(using: .utf8), textData.count <= maxClipboardTextBytes else {
        result(FlutterError(code: "Clipboard is too large", message: nil, details: nil))
        return
      }
      item["public.utf8-plain-text"] = text
    }
    if let typedData = arguments["png"] as? FlutterStandardTypedData,
       !typedData.data.isEmpty {
      let png = typedData.data
      guard isValidClipboardPng(png), UIImage(data: png) != nil else {
        result(FlutterError(code: "Clipboard image is invalid", message: nil, details: nil))
        return
      }
      item["public.png"] = png
    }

    guard !item.isEmpty else {
      result(FlutterError(code: "Clipboard is empty", message: nil, details: nil))
      return
    }
    UIPasteboard.general.setItems(
      [item],
      options: [UIPasteboard.OptionsKey.localOnly: true]
    )
    result(true)
  }

  // Validate the cheap, fixed-size PNG header before asking UIKit to decode it.
  // This prevents a small compressed payload from advertising dimensions large
  // enough to cause disproportionate memory use during clipboard handling.
  private func isValidClipboardPng(_ data: Data) -> Bool {
    guard data.count >= 24, data.count <= maxClipboardPngBytes else {
      return false
    }
    let bytes = [UInt8](data.prefix(24))
    guard Array(bytes[0..<8]) == [137, 80, 78, 71, 13, 10, 26, 10],
          Array(bytes[12..<16]) == [73, 72, 68, 82] else {
      return false
    }
    let width = bytes[16..<20].reduce(UInt64(0)) { ($0 << 8) | UInt64($1) }
    let height = bytes[20..<24].reduce(UInt64(0)) { ($0 << 8) | UInt64($1) }
    guard width > 0, height > 0, width <= maxClipboardImagePixels else {
      return false
    }
    return height <= maxClipboardImagePixels / width
  }
    
  public func dummyMethodToEnforceBundling() {
      dummy_method_to_enforce_bundling();
    session_get_rgba(nil, 0);
  }
}
