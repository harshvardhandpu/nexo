import { ShieldCheck, ShieldQuestion, X } from "lucide-react";
import type { PairingInfo } from "../api/desktop";
import { NeonButton } from "./ui";

/**
 * Fingerprint confirmation modal for device pairing. Shown after
 * `start_pairing` resolves the discovered device's advertised certificate
 * fingerprint. No trust is stored until the user confirms here — rejecting
 * (Cancel) discards the pending pairing on the backend.
 */
export function PairingDialog({
  pairing,
  busy,
  onConfirm,
  onReject,
}: {
  pairing: PairingInfo;
  busy?: boolean;
  onConfirm: () => void;
  onReject: () => void;
}) {
  return (
    <div className="modal-backdrop" onClick={busy ? undefined : onReject}>
      <div
        className="modal glass glass--strong"
        role="dialog"
        aria-modal="true"
        aria-label="Confirm device fingerprint"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="modal__glyph">
          <ShieldQuestion size={26} />
        </div>
        <h3 className="modal__title">Verify this device</h3>
        <p className="modal__sub text-muted">
          Confirm the fingerprint below matches the one shown on{" "}
          <strong>{pairing.displayName}</strong>. Only trust the device if they
          match — this is what keeps an impostor off your network.
        </p>

        <div className="modal__rows">
          <div className="kv">
            <span className="kv__k">Device</span>
            <span className="kv__v">{pairing.displayName}</span>
          </div>
          <div className="kv">
            <span className="kv__k">Address</span>
            <span className="kv__v mono">{pairing.address}</span>
          </div>
          <div className="kv">
            <span className="kv__k">Fingerprint</span>
            <span className="kv__v mono">SHA256:{pairing.fingerprint}</span>
          </div>
        </div>

        {pairing.alreadyTrusted ? (
          <p className="modal__sub text-muted">
            This device is already trusted — confirming updates its stored entry.
          </p>
        ) : null}

        <div className="modal__actions">
          <NeonButton variant="ghost" icon={X} onClick={onReject} disabled={busy}>
            Cancel
          </NeonButton>
          <NeonButton icon={ShieldCheck} onClick={onConfirm} loading={busy}>
            Trust device
          </NeonButton>
        </div>
      </div>
    </div>
  );
}
