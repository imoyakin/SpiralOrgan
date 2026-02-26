import Cocoa
import FlutterMacOS
import Darwin

private final class SpiralOrganCoreBridge {
  typealias StartFn = @convention(c) () -> Bool
  typealias InvokeFn = @convention(c) (UnsafePointer<CChar>) -> UnsafeMutablePointer<CChar>?
  typealias EventSubscribeFn = @convention(c) (UnsafePointer<CChar>) -> UnsafeMutablePointer<CChar>?
  typealias EventNextFn = @convention(c) (UnsafePointer<CChar>) -> UnsafeMutablePointer<CChar>?
  typealias EventUnsubscribeFn = @convention(c) (UInt64) -> Bool
  typealias FreeFn = @convention(c) (UnsafeMutablePointer<CChar>?) -> Void

  private var handle: UnsafeMutableRawPointer?
  private var startFn: StartFn?
  private var invokeFn: InvokeFn?
  private var eventSubscribeFn: EventSubscribeFn?
  private var eventNextFn: EventNextFn?
  private var eventUnsubscribeFn: EventUnsubscribeFn?
  private var freeFn: FreeFn?

  func start() throws -> Bool {
    if handle == nil {
      let resolvedPath = try resolveLibraryPath()
      guard let loaded = dlopen(resolvedPath, RTLD_NOW | RTLD_LOCAL) else {
        throw NSError(
          domain: "spiral_organ_core",
          code: 1001,
          userInfo: [NSLocalizedDescriptionKey: "dlopen failed for \(resolvedPath): \(lastDlError())"]
        )
      }
      handle = loaded
      try loadSymbols(from: loaded)
    }

    guard let startFn else {
      throw NSError(
        domain: "spiral_organ_core",
        code: 1002,
        userInfo: [NSLocalizedDescriptionKey: "spiral_organ_core_start symbol is not loaded"]
      )
    }

    return startFn()
  }

  func invoke(method: String, path: String, body: Any?) throws -> [String: Any] {
    guard let invokeFn, let freeFn else {
      throw NSError(
        domain: "spiral_organ_core",
        code: 1003,
        userInfo: [NSLocalizedDescriptionKey: "core bridge not started. call core.start first"]
      )
    }

    let request: [String: Any] = [
      "method": method,
      "path": path,
      "body": body as Any
    ]
    let requestData = try JSONSerialization.data(withJSONObject: request)
    guard let requestString = String(data: requestData, encoding: .utf8) else {
      throw NSError(
        domain: "spiral_organ_core",
        code: 1004,
        userInfo: [NSLocalizedDescriptionKey: "failed to encode request as utf8"]
      )
    }

    let responsePtr = try requestString.withCString { cstr in
      guard let ptr = invokeFn(cstr) else {
        throw NSError(
          domain: "spiral_organ_core",
          code: 1005,
          userInfo: [NSLocalizedDescriptionKey: "spiral_organ_core_invoke returned null"]
        )
      }
      return ptr
    }
    defer { freeFn(responsePtr) }

    let responseString = String(cString: responsePtr)
    let responseData = Data(responseString.utf8)
    let decoded = try JSONSerialization.jsonObject(with: responseData)
    guard let payload = decoded as? [String: Any] else {
      throw NSError(
        domain: "spiral_organ_core",
        code: 1006,
        userInfo: [NSLocalizedDescriptionKey: "invalid core response payload"]
      )
    }

    let ok = payload["ok"] as? Bool ?? false
    if ok {
      return payload["data"] as? [String: Any] ?? [:]
    }

    let message = payload["error"] as? String ?? "core invoke failed"
    throw NSError(
      domain: "spiral_organ_core",
      code: 1007,
      userInfo: [NSLocalizedDescriptionKey: message]
    )
  }

  func eventSubscribe(sessionId: String?, taskId: String?) throws -> UInt64 {
    guard let eventSubscribeFn, let freeFn else {
      throw NSError(
        domain: "spiral_organ_core",
        code: 1008,
        userInfo: [NSLocalizedDescriptionKey: "event subscribe symbol is not loaded"]
      )
    }

    let request: [String: Any] = [
      "session_id": sessionId as Any,
      "task_id": taskId as Any
    ]
    let requestData = try JSONSerialization.data(withJSONObject: request)
    guard let requestString = String(data: requestData, encoding: .utf8) else {
      throw NSError(
        domain: "spiral_organ_core",
        code: 1009,
        userInfo: [NSLocalizedDescriptionKey: "failed to encode event subscribe request as utf8"]
      )
    }

    let responsePtr = try requestString.withCString { cstr in
      guard let ptr = eventSubscribeFn(cstr) else {
        throw NSError(
          domain: "spiral_organ_core",
          code: 1014,
          userInfo: [NSLocalizedDescriptionKey: "spiral_organ_core_event_subscribe returned null"]
        )
      }
      return ptr
    }
    defer { freeFn(responsePtr) }

    let payload = try decodeCoreEnvelope(responsePtr: responsePtr, errorCode: 1015)
    if let streamId = payload["stream_id"] as? UInt64 {
      return streamId
    }
    if let streamId = payload["stream_id"] as? NSNumber {
      return streamId.uint64Value
    }
    throw NSError(
      domain: "spiral_organ_core",
      code: 1016,
      userInfo: [NSLocalizedDescriptionKey: "event subscribe response missing stream_id"]
    )
  }

  func eventNext(streamId: UInt64, timeoutMs: UInt64) throws -> [String: Any] {
    guard let eventNextFn, let freeFn else {
      throw NSError(
        domain: "spiral_organ_core",
        code: 1017,
        userInfo: [NSLocalizedDescriptionKey: "event next symbol is not loaded"]
      )
    }

    let request: [String: Any] = [
      "stream_id": streamId,
      "timeout_ms": timeoutMs
    ]
    let requestData = try JSONSerialization.data(withJSONObject: request)
    guard let requestString = String(data: requestData, encoding: .utf8) else {
      throw NSError(
        domain: "spiral_organ_core",
        code: 1018,
        userInfo: [NSLocalizedDescriptionKey: "failed to encode event next request as utf8"]
      )
    }

    let responsePtr = try requestString.withCString { cstr in
      guard let ptr = eventNextFn(cstr) else {
        throw NSError(
          domain: "spiral_organ_core",
          code: 1019,
          userInfo: [NSLocalizedDescriptionKey: "spiral_organ_core_event_next returned null"]
        )
      }
      return ptr
    }
    defer { freeFn(responsePtr) }
    return try decodeCoreEnvelope(responsePtr: responsePtr, errorCode: 1020)
  }

  func eventUnsubscribe(streamId: UInt64) {
    guard let eventUnsubscribeFn else {
      return
    }
    _ = eventUnsubscribeFn(streamId)
  }

  private func decodeCoreEnvelope(
    responsePtr: UnsafeMutablePointer<CChar>,
    errorCode: Int
  ) throws -> [String: Any] {
    let responseString = String(cString: responsePtr)
    let responseData = Data(responseString.utf8)
    let decoded = try JSONSerialization.jsonObject(with: responseData)
    guard let envelope = decoded as? [String: Any] else {
      throw NSError(
        domain: "spiral_organ_core",
        code: errorCode,
        userInfo: [NSLocalizedDescriptionKey: "invalid core response payload"]
      )
    }

    let ok = envelope["ok"] as? Bool ?? false
    if ok {
      return envelope["data"] as? [String: Any] ?? [:]
    }

    let message = envelope["error"] as? String ?? "core bridge error"
    throw NSError(
      domain: "spiral_organ_core",
      code: errorCode + 1,
      userInfo: [NSLocalizedDescriptionKey: message]
    )
  }

  private func loadSymbols(from handle: UnsafeMutableRawPointer) throws {
    guard let startSym = dlsym(handle, "spiral_organ_core_start") else {
      throw NSError(
        domain: "spiral_organ_core",
        code: 1010,
        userInfo: [NSLocalizedDescriptionKey: "missing symbol spiral_organ_core_start"]
      )
    }
    guard let invokeSym = dlsym(handle, "spiral_organ_core_invoke") else {
      throw NSError(
        domain: "spiral_organ_core",
        code: 1011,
        userInfo: [NSLocalizedDescriptionKey: "missing symbol spiral_organ_core_invoke"]
      )
    }
    guard let eventSubscribeSym = dlsym(handle, "spiral_organ_core_event_subscribe") else {
      throw NSError(
        domain: "spiral_organ_core",
        code: 1021,
        userInfo: [NSLocalizedDescriptionKey: "missing symbol spiral_organ_core_event_subscribe"]
      )
    }
    guard let eventNextSym = dlsym(handle, "spiral_organ_core_event_next") else {
      throw NSError(
        domain: "spiral_organ_core",
        code: 1022,
        userInfo: [NSLocalizedDescriptionKey: "missing symbol spiral_organ_core_event_next"]
      )
    }
    guard let eventUnsubscribeSym = dlsym(handle, "spiral_organ_core_event_unsubscribe") else {
      throw NSError(
        domain: "spiral_organ_core",
        code: 1023,
        userInfo: [NSLocalizedDescriptionKey: "missing symbol spiral_organ_core_event_unsubscribe"]
      )
    }
    guard let freeSym = dlsym(handle, "spiral_organ_core_free_string") else {
      throw NSError(
        domain: "spiral_organ_core",
        code: 1012,
        userInfo: [NSLocalizedDescriptionKey: "missing symbol spiral_organ_core_free_string"]
      )
    }

    startFn = unsafeBitCast(startSym, to: StartFn.self)
    invokeFn = unsafeBitCast(invokeSym, to: InvokeFn.self)
    eventSubscribeFn = unsafeBitCast(eventSubscribeSym, to: EventSubscribeFn.self)
    eventNextFn = unsafeBitCast(eventNextSym, to: EventNextFn.self)
    eventUnsubscribeFn = unsafeBitCast(eventUnsubscribeSym, to: EventUnsubscribeFn.self)
    freeFn = unsafeBitCast(freeSym, to: FreeFn.self)
  }

  private func resolveLibraryPath() throws -> String {
    let fileManager = FileManager.default

    if let frameworksPath = Bundle.main.privateFrameworksPath {
      let embedded = URL(fileURLWithPath: frameworksPath, isDirectory: true)
        .appendingPathComponent("libspiral_organ_core.dylib", isDirectory: false)
      if fileManager.fileExists(atPath: embedded.path) {
        return embedded.path
      }
    }

    // Local development fallback if the app bundle has not embedded the library.
    var cursor = URL(fileURLWithPath: fileManager.currentDirectoryPath, isDirectory: true)

    for _ in 0..<10 {
      let candidate = cursor
        .appendingPathComponent("target", isDirectory: true)
        .appendingPathComponent("debug", isDirectory: true)
        .appendingPathComponent("libspiral_organ_core.dylib", isDirectory: false)
      if fileManager.fileExists(atPath: candidate.path) {
        return candidate.path
      }
      let parent = cursor.deletingLastPathComponent()
      if parent.path == cursor.path {
        break
      }
      cursor = parent
    }

    throw NSError(
      domain: "spiral_organ_core",
      code: 1013,
      userInfo: [
        NSLocalizedDescriptionKey:
          "unable to find embedded libspiral_organ_core.dylib in app Frameworks directory"
      ]
    )
  }

  private func lastDlError() -> String {
    guard let err = dlerror() else {
      return "unknown dlopen error"
    }
    return String(cString: err)
  }
}

private final class CoreEventStreamHandler: NSObject, FlutterStreamHandler {
  private let bridge: SpiralOrganCoreBridge
  private let eventQueue = DispatchQueue(
    label: "spiral_organ.core.events",
    qos: .userInitiated
  )
  private let lock = NSLock()
  private var sink: FlutterEventSink?
  private var streamId: UInt64?
  private var token: UUID?

  init(bridge: SpiralOrganCoreBridge) {
    self.bridge = bridge
  }

  func onListen(withArguments arguments: Any?, eventSink events: @escaping FlutterEventSink)
    -> FlutterError?
  {
    do {
      _ = try bridge.start()
      let args = arguments as? [String: Any]
      let sessionId = args?["session_id"] as? String
      let taskId = args?["task_id"] as? String
      let streamId = try bridge.eventSubscribe(sessionId: sessionId, taskId: taskId)
      let token = UUID()
      lock.lock()
      self.sink = events
      self.streamId = streamId
      self.token = token
      lock.unlock()

      eventQueue.async { [weak self] in
        self?.pumpEvents(streamId: streamId, token: token)
      }
      return nil
    } catch {
      return FlutterError(
        code: "core_event_error",
        message: error.localizedDescription,
        details: nil
      )
    }
  }

  func onCancel(withArguments arguments: Any?) -> FlutterError? {
    stopCurrentStream()
    return nil
  }

  private func stopCurrentStream() {
    lock.lock()
    let streamId = self.streamId
    self.streamId = nil
    self.token = nil
    self.sink = nil
    lock.unlock()

    if let streamId {
      bridge.eventUnsubscribe(streamId: streamId)
    }
  }

  private func pumpEvents(streamId: UInt64, token: UUID) {
    while isActive(token: token, streamId: streamId) {
      do {
        let payload = try bridge.eventNext(streamId: streamId, timeoutMs: 30_000)
        if (payload["timeout"] as? Bool) == true {
          continue
        }
        DispatchQueue.main.async { [weak self] in
          guard let self else { return }
          guard let sink = self.activeSink(token: token, streamId: streamId) else { return }
          sink(payload)
        }
      } catch {
        DispatchQueue.main.async { [weak self] in
          guard let self else { return }
          guard let sink = self.activeSink(token: token, streamId: streamId) else { return }
          sink(
            FlutterError(
              code: "core_event_error",
              message: error.localizedDescription,
              details: nil
            )
          )
        }
        break
      }
    }

    bridge.eventUnsubscribe(streamId: streamId)
  }

  private func isActive(token: UUID, streamId: UInt64) -> Bool {
    lock.lock()
    defer { lock.unlock() }
    return self.token == token && self.streamId == streamId
  }

  private func activeSink(token: UUID, streamId: UInt64) -> FlutterEventSink? {
    lock.lock()
    defer { lock.unlock() }
    guard self.token == token && self.streamId == streamId else {
      return nil
    }
    return self.sink
  }
}

class MainFlutterWindow: NSWindow {
  private var coreChannel: FlutterMethodChannel?
  private var eventChannel: FlutterEventChannel?
  private var eventStreamHandler: CoreEventStreamHandler?
  private let coreBridge = SpiralOrganCoreBridge()
  private let coreInvokeQueue = DispatchQueue(
    label: "spiral_organ.core.invoke",
    qos: .userInitiated
  )

  override func awakeFromNib() {
    let flutterViewController = FlutterViewController()
    let windowFrame = self.frame
    self.contentViewController = flutterViewController
    self.setFrame(windowFrame, display: true)

    RegisterGeneratedPlugins(registry: flutterViewController)

    let channel = FlutterMethodChannel(
      name: "spiral_organ/core",
      binaryMessenger: flutterViewController.engine.binaryMessenger
    )
    channel.setMethodCallHandler { [weak self] call, result in
      self?.handleCoreChannel(call: call, result: result)
    }
    coreChannel = channel

    let eventChannel = FlutterEventChannel(
      name: "spiral_organ/events",
      binaryMessenger: flutterViewController.engine.binaryMessenger
    )
    let eventHandler = CoreEventStreamHandler(bridge: coreBridge)
    eventChannel.setStreamHandler(eventHandler)
    self.eventChannel = eventChannel
    eventStreamHandler = eventHandler

    super.awakeFromNib()
  }

  private func handleCoreChannel(call: FlutterMethodCall, result: @escaping FlutterResult) {
    switch call.method {
    case "core.start":
      do {
        let started = try coreBridge.start()
        result(started)
      } catch {
        result(
          FlutterError(
            code: "core_bridge_error",
            message: error.localizedDescription,
            details: nil
          )
        )
      }
    case "kernel.invoke":
      guard let args = call.arguments as? [String: Any],
            let method = args["method"] as? String,
            let path = args["path"] as? String else {
        result(
          FlutterError(
            code: "bad_args",
            message: "kernel.invoke requires method and path",
            details: nil
          )
        )
        return
      }

      let body = args["body"]
      coreInvokeQueue.async { [weak self] in
        guard let self else {
          DispatchQueue.main.async {
            result(
              FlutterError(
                code: "core_bridge_error",
                message: "core bridge unavailable",
                details: nil
              )
            )
          }
          return
        }

        do {
          let response = try self.coreBridge.invoke(method: method, path: path, body: body)
          DispatchQueue.main.async {
            result(response)
          }
        } catch {
          DispatchQueue.main.async {
            result(
              FlutterError(
                code: "core_bridge_error",
                message: error.localizedDescription,
                details: nil
              )
            )
          }
        }
      }
    default:
      result(FlutterMethodNotImplemented)
    }
  }
}
