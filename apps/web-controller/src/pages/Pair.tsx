import { useEffect, useMemo, useRef, useState } from "react";
import { useLocation, useNavigate, useParams } from "react-router-dom";
import {
  parsePairUrl,
  redeemPairing,
  PairError,
  type PairUrl,
} from "../lib/pairing";
import { startQrScan, type QrScanner } from "../lib/qr-scan";
import { savePairing } from "../lib/storage/idb";

// Camera scan needs `navigator.mediaDevices.getUserMedia`, which browsers
// gate behind a secure context (HTTPS, localhost, or file://). On plain
// HTTP over a tailnet IP, `getUserMedia` is undefined. We hide the Start
// scan button in that case and tell the user to use the URL paste below
// (the primary flow is "phone OS camera scans QR -> opens URL", so the
// in-app camera scan is a fallback anyway).
function cameraScanAvailable(): boolean {
  if (typeof window === "undefined") return false;
  if (typeof navigator === "undefined") return false;
  const md = (navigator as Navigator).mediaDevices;
  if (!md || typeof md.getUserMedia !== "function") return false;
  // Secure-context check: covers HTTPS, localhost, and file:// per the spec.
  if (window.isSecureContext === false) return false;
  return true;
}

function detectPlatform(ua: string): string {
  if (/iPhone|iPad|iPod/i.test(ua)) return "ios";
  if (/Android/i.test(ua)) return "android";
  if (/Macintosh|Windows|Linux/i.test(ua)) return "desktop";
  return "web";
}

function defaultDeviceName(ua: string): string {
  if (/iPhone/i.test(ua)) return "iPhone";
  if (/iPad/i.test(ua)) return "iPad";
  if (/Android/i.test(ua)) return "Android device";
  if (/Macintosh/i.test(ua)) return "Mac";
  if (/Windows/i.test(ua)) return "Windows PC";
  if (/Linux/i.test(ua)) return "Linux PC";
  return "Web device";
}

function friendlyError(code: string): string {
  switch (code) {
    case "token_expired":
      return "This pairing token has expired. Ask the host to run `omw pair qr` again.";
    case "token_already_used":
      return "This pairing token has already been redeemed. Ask the host for a fresh one.";
    case "token_unknown":
      return "The host doesn't recognize this pairing token. Make sure you're pairing with the right machine.";
    case "invalid_pubkey":
      return "Pairing failed: the device key was rejected by the host.";
    case "invalid_body":
      return "Pairing failed: the request was malformed.";
    case "network_error":
      return "Couldn't reach the host. Check your network and that the host is running `omw remote start`.";
    case "bad_response":
      return "The host returned an unexpected response. Check that the host is up to date.";
    default:
      return `Pairing failed (${code}).`;
  }
}

export default function Pair() {
  const { t: pathToken } = useParams();
  const location = useLocation();
  const navigate = useNavigate();

  const ua = typeof navigator !== "undefined" ? navigator.userAgent : "";

  // The pair URL the host emits is `<origin>/pair?t=<token>` — the token is
  // a query param. `useParams()` only reaches `/pair/:t` (path), so we ALSO
  // pull `?t=` from `useLocation().search` here. Either source works.
  const queryToken = useMemo(() => {
    const params = new URLSearchParams(location.search);
    return params.get("t") ?? "";
  }, [location.search]);
  const urlToken = pathToken || queryToken;

  const [pasted, setPasted] = useState<string>("");
  const [scannedUrl, setScannedUrl] = useState<string>("");
  const [deviceName, setDeviceName] = useState<string>(defaultDeviceName(ua));
  const platform = useMemo(() => detectPlatform(ua), [ua]);

  const [scanning, setScanning] = useState(false);
  const [redeeming, setRedeeming] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const cameraOk = useMemo(cameraScanAvailable, []);
  // Auto-redeem fires exactly once when we land with a URL token. Subsequent
  // re-renders shouldn't retry (e.g., after an error the user might edit the
  // device name). The user can always click Pair manually.
  const autoTriedRef = useRef(false);

  const videoRef = useRef<HTMLVideoElement | null>(null);
  const scannerRef = useRef<QrScanner | null>(null);

  // The textarea reflects either pasted or scanned URL; pre-fill when arriving
  // via /pair?t=<token> or /pair/:t.
  const textValue = pasted || scannedUrl;

  // Resolve the pair URL from one of: pasted text, scanned text,
  // or the URL-encoded token (synthetic URL using the current origin).
  const pairUrl: PairUrl | null = useMemo(() => {
    if (textValue.trim().length > 0) {
      return parsePairUrl(textValue.trim());
    }
    if (urlToken && typeof window !== "undefined") {
      const origin = window.location.origin;
      return parsePairUrl(`${origin}/pair?t=${urlToken}`);
    }
    return null;
  }, [textValue, urlToken]);

  useEffect(() => {
    return () => {
      if (scannerRef.current) {
        scannerRef.current.stop();
        scannerRef.current = null;
      }
    };
  }, []);

  async function handleStartScan() {
    setErrorMsg(null);
    if (!videoRef.current) return;
    try {
      setScanning(true);
      const scanner = await startQrScan(
        videoRef.current,
        (text) => {
          setScannedUrl(text);
          setPasted("");
          // Stop after first successful decode.
          if (scannerRef.current) {
            scannerRef.current.stop();
            scannerRef.current = null;
          }
          setScanning(false);
        },
        (err) => {
          setErrorMsg(`Scan error: ${err.message}`);
        },
      );
      scannerRef.current = scanner;
    } catch (e) {
      setScanning(false);
      setErrorMsg(
        `Couldn't start camera: ${e instanceof Error ? e.message : String(e)}`,
      );
    }
  }

  function handleStopScan() {
    if (scannerRef.current) {
      scannerRef.current.stop();
      scannerRef.current = null;
    }
    setScanning(false);
  }

  async function handlePair() {
    if (!pairUrl) return;
    setErrorMsg(null);
    setRedeeming(true);
    try {
      const result = await redeemPairing(pairUrl, deviceName.trim(), platform);
      const pairingRecord = {
        hostId: result.hostId,
        hostUrl: result.hostUrl,
        hostPubkey: result.hostPubkey,
        deviceId: result.deviceId,
        privateKeyJwk: result.privateKeyJwk,
        capabilityTokenB64: result.capabilityTokenB64,
        pairedAt: new Date().toISOString(),
        capabilities: result.capabilities,
      };
      await savePairing(pairingRecord);

      navigate(`/host/${encodeURIComponent(result.hostId)}`);
    } catch (e) {
      if (e instanceof PairError) {
        setErrorMsg(friendlyError(e.code));
      } else {
        setErrorMsg(
          `Pairing failed: ${e instanceof Error ? e.message : String(e)}`,
        );
      }
    } finally {
      setRedeeming(false);
    }
  }

  const canPair = !!pairUrl && !redeeming && deviceName.trim().length > 0;

  // Auto-redeem when we landed with a URL token (the user opened
  // <origin>/pair?t=<token> from the QR code or shared link). Fires once,
  // when the page is otherwise idle. The default device name is good enough;
  // the user can rename later from settings.
  useEffect(() => {
    if (autoTriedRef.current) return;
    if (!urlToken) return;
    if (!pairUrl) return;
    if (redeeming) return;
    if (errorMsg) return;
    autoTriedRef.current = true;
    void handlePair();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [urlToken, pairUrl]);

  return (
    <section className="max-w-2xl mx-auto space-y-6">
      <h1 className="text-2xl font-semibold">Pair</h1>

      <div className="rounded-lg border border-neutral-800 bg-neutral-900/40 p-4 space-y-4">
        {cameraOk ? (
          <div>
            <h2 className="text-sm font-semibold uppercase tracking-wide text-neutral-300">
              Scan QR
            </h2>
            <p className="text-xs text-neutral-500 mt-1">
              Point your camera at the QR shown by `omw pair qr` on the host.
            </p>
            <div className="mt-3 space-y-2">
              <video
                ref={videoRef}
                className={`w-full max-w-sm rounded bg-black ${
                  scanning ? "block" : "hidden"
                }`}
                muted
                playsInline
              />
              <div className="flex gap-2">
                {!scanning ? (
                  <button
                    type="button"
                    onClick={handleStartScan}
                    className="px-3 py-1.5 rounded bg-neutral-800 hover:bg-neutral-700 text-sm"
                  >
                    Start scan
                  </button>
                ) : (
                  <button
                    type="button"
                    onClick={handleStopScan}
                    className="px-3 py-1.5 rounded bg-neutral-800 hover:bg-neutral-700 text-sm"
                  >
                    Stop scan
                  </button>
                )}
              </div>
            </div>
          </div>
        ) : (
          <div>
            <h2 className="text-sm font-semibold uppercase tracking-wide text-neutral-300">
              In-app QR scan unavailable
            </h2>
            <p className="text-xs text-neutral-500 mt-1">
              Your browser only allows camera access on a secure origin
              (HTTPS). Open the host&apos;s pair URL directly (the page
              auto-pairs from <code>?t=</code>), or paste the URL below.
            </p>
          </div>
        )}

        <div className="border-t border-neutral-800 pt-4">
          <h2 className="text-sm font-semibold uppercase tracking-wide text-neutral-300">
            Or paste the pairing URL
          </h2>
          <textarea
            aria-label="Pairing URL"
            value={textValue}
            onChange={(e) => {
              setPasted(e.target.value);
              setScannedUrl("");
            }}
            placeholder="https://hostname.tailnet.ts.net/pair?t=..."
            className="mt-2 w-full h-24 rounded bg-neutral-950 border border-neutral-800 p-2 text-sm font-mono"
          />
        </div>

        <div className="border-t border-neutral-800 pt-4">
          <label className="block text-sm">
            <span className="text-neutral-300">Device name</span>
            <input
              type="text"
              value={deviceName}
              onChange={(e) => setDeviceName(e.target.value)}
              className="mt-1 w-full rounded bg-neutral-950 border border-neutral-800 p-2 text-sm"
            />
          </label>
          <p className="text-xs text-neutral-500 mt-1">
            Platform: <span className="font-mono">{platform}</span>
          </p>
        </div>

        {errorMsg && (
          <div
            role="alert"
            className="rounded border border-red-700 bg-red-900/30 p-3 text-sm text-red-200"
          >
            {errorMsg}
          </div>
        )}

        <div className="flex items-center gap-3 pt-2">
          <button
            type="button"
            onClick={handlePair}
            disabled={!canPair}
            className="px-4 py-2 rounded bg-blue-600 hover:bg-blue-500 disabled:bg-neutral-800 disabled:text-neutral-500 text-sm font-semibold"
          >
            {redeeming ? "Redeeming…" : "Pair"}
          </button>
          {redeeming && (
            <span className="text-xs text-neutral-400">
              Talking to the host…
            </span>
          )}
        </div>
      </div>
    </section>
  );
}
