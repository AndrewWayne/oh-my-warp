// Camera-driven QR scanning. Wraps @zxing/browser with a small
// start/stop interface. Requests the rear camera on phones via
// facingMode:"environment" with a fallback to the default device.

import { BrowserQRCodeReader } from "@zxing/browser";

export interface QrScanner {
  stop(): void;
}

export async function startQrScan(
  videoElement: HTMLVideoElement,
  onResult: (text: string) => void,
  onError?: (err: Error) => void,
): Promise<QrScanner> {
  const reader = new BrowserQRCodeReader();

  let stream: MediaStream | null = null;
  try {
    stream = await navigator.mediaDevices.getUserMedia({
      video: { facingMode: { ideal: "environment" } },
      audio: false,
    });
  } catch {
    // Fall back to default camera.
    stream = await navigator.mediaDevices.getUserMedia({
      video: true,
      audio: false,
    });
  }

  videoElement.srcObject = stream;
  // Some browsers need an explicit play() after setting srcObject.
  try {
    await videoElement.play();
  } catch {
    /* ignore — autoplay may be blocked but decode often still works */
  }

  let stopped = false;
  const controls = await reader.decodeFromVideoElement(
    videoElement,
    (result, err) => {
      if (stopped) return;
      if (result) {
        onResult(result.getText());
      } else if (err && onError) {
        // zxing emits NotFoundException on every empty frame — ignore.
        const name = (err as { name?: string }).name ?? "";
        if (name && name !== "NotFoundException") {
          onError(err as Error);
        }
      }
    },
  );

  return {
    stop() {
      stopped = true;
      try {
        controls.stop();
      } catch {
        /* ignore */
      }
      if (stream) {
        for (const track of stream.getTracks()) {
          try {
            track.stop();
          } catch {
            /* ignore */
          }
        }
      }
      try {
        videoElement.srcObject = null;
      } catch {
        /* ignore */
      }
    },
  };
}
