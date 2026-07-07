import { useEffect, useState } from "react";
import {
  ArrowRight,
  Check,
  CloudOff,
  HardDriveDownload,
  Lock,
  Radio,
  ShieldCheck,
  Sparkles,
  Wifi,
  Zap,
} from "lucide-react";
import {
  type IdentityPreview,
  type SelfCheck,
  completeOnboarding,
  previewIdentity,
  runSelfCheck,
} from "../api/desktop";
import { NeonButton, Toggle } from "../components/ui";
import { NodeNetwork } from "../components/NodeNetwork";

function CheckRow({ ok, label }: { ok: boolean; label: string }) {
  return (
    <div className="onboarding__check-row">
      <span
        className={`presence-dot ${ok ? "presence-dot--online" : ""}`}
        style={{ position: "relative", inset: 0, width: 10, height: 10 }}
      />
      <span className={ok ? "" : "text-faint"}>{label}</span>
      <span className={ok ? "" : "text-faint"} style={{ marginLeft: "auto" }}>
        {ok ? "OK" : "—"}
      </span>
    </div>
  );
}

/**
 * Feature 3: first-launch onboarding. Three steps — welcome, device setup,
 * ready — then persists the choices and marks onboarding complete so it never
 * shows again. Rendered full-screen above everything until finished.
 */
export function Onboarding({ onDone }: { onDone: () => void }) {
  const [step, setStep] = useState(0);
  const [deviceName, setDeviceName] = useState("");
  const [discoverable, setDiscoverable] = useState(true);
  const [background, setBackground] = useState(true);
  const [startOnLogin, setStartOnLogin] = useState(false);
  const [busy, setBusy] = useState(false);
  const [identity, setIdentity] = useState<IdentityPreview | null>(null);
  const [check, setCheck] = useState<SelfCheck | null>(null);
  const [checking, setChecking] = useState(false);

  useEffect(() => {
    void previewIdentity().then(setIdentity).catch(() => {});
  }, []);

  const previewName = deviceName.trim() || identity?.displayName || "This device";

  const testConnection = async () => {
    setChecking(true);
    try {
      setCheck(await runSelfCheck());
    } catch {
      setCheck(null);
    } finally {
      setChecking(false);
    }
  };

  const finish = async () => {
    setBusy(true);
    try {
      await completeOnboarding(
        deviceName.trim(),
        discoverable,
        background,
        startOnLogin,
      );
      onDone();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="onboarding">
      <div className="onboarding__card glass glass--strong">
        <div className="onboarding__dots">
          {[0, 1, 2].map((index) => (
            <span
              key={index}
              className={`onboarding__dot ${index === step ? "is-active" : ""} ${
                index < step ? "is-done" : ""
              }`}
            />
          ))}
        </div>

        {step === 0 ? (
          <div className="onboarding__body" key="s0">
            <div className="onboarding__viz">
              <NodeNetwork live />
            </div>
            <div className="onboarding__glyph">
              <Sparkles size={26} />
            </div>
            <h1 className="onboarding__title">
              Welcome to <span className="gradient-text">Nexo</span>
            </h1>
            <p className="onboarding__lead">
              Encrypted peer-to-peer file transfers. No cloud, no accounts — your
              files go directly device to device, secured end to end.
            </p>
            <div className="onboarding__security">
              <div className="onboarding__security-item">
                <Lock size={16} /> End-to-end encrypted
              </div>
              <div className="onboarding__security-item">
                <CloudOff size={16} /> No cloud server
              </div>
              <div className="onboarding__security-item">
                <HardDriveDownload size={16} /> Files stay on your devices
              </div>
            </div>
            <NeonButton icon={ArrowRight} onClick={() => setStep(1)}>
              Get started
            </NeonButton>
          </div>
        ) : null}

        {step === 1 ? (
          <div className="onboarding__body" key="s1">
            <div className="onboarding__glyph">
              <Wifi size={24} />
            </div>
            <h1 className="onboarding__title">Set up this device</h1>
            <p className="onboarding__lead">
              Choose how Nexo appears to nearby devices.
            </p>
            <label className="field" style={{ width: "100%", textAlign: "left" }}>
              <span>Device name</span>
              <input
                className="input"
                placeholder="e.g. Harsh Laptop"
                value={deviceName}
                onChange={(event) => setDeviceName(event.target.value)}
                autoFocus
              />
            </label>

            <div className="onboarding__identity">
              <span className="text-faint">Nexo will identify this device as</span>
              <strong className="gradient-text">{previewName}</strong>
              <span className="mono text-muted">
                {identity?.address ?? "no LAN address detected"}
              </span>
            </div>

            <div className="onboarding__toggles">
              <Toggle
                label="Discoverable"
                hint="Let nearby devices find this one over the local network."
                checked={discoverable}
                onChange={setDiscoverable}
              />
              <div className="divider" />
              <Toggle
                label="Background receiver"
                hint="Stay available to receive after the window is closed."
                checked={background}
                onChange={setBackground}
              />
              <div className="divider" />
              <Toggle
                label="Start on login"
                hint="Launch Nexo automatically when you sign in."
                checked={startOnLogin}
                onChange={setStartOnLogin}
              />
            </div>

            <div className="onboarding__check">
              <NeonButton
                variant="ghost"
                icon={Zap}
                onClick={testConnection}
                loading={checking}
                block
              >
                Test connection
              </NeonButton>
              {check ? (
                <div className="onboarding__check-rows">
                  <CheckRow ok={check.storageWritable} label="Storage writable" />
                  <CheckRow ok={check.receiverReady} label="Receiver available" />
                  <CheckRow
                    ok={check.discoveryEnabled}
                    label="Discovery enabled"
                  />
                </div>
              ) : null}
            </div>

            <div className="row" style={{ gap: 10, width: "100%" }}>
              <NeonButton variant="ghost" onClick={() => setStep(0)}>
                Back
              </NeonButton>
              <NeonButton
                icon={ArrowRight}
                onClick={() => setStep(2)}
                block
              >
                Continue
              </NeonButton>
            </div>
          </div>
        ) : null}

        {step === 2 ? (
          <div className="onboarding__body" key="s2">
            <div className="onboarding__glyph onboarding__glyph--ok">
              <ShieldCheck size={26} />
            </div>
            <h1 className="onboarding__title">Your device is ready</h1>
            <p className="onboarding__lead">
              {deviceName.trim() ? (
                <>
                  <strong>{deviceName.trim()}</strong> is set up.{" "}
                </>
              ) : null}
              {discoverable
                ? "It’s discoverable and "
                : "Discovery is off and "}
              {background
                ? "will keep receiving in the background."
                : "will receive only while open."}
            </p>
            <div className="row row--wrap" style={{ gap: 8, justifyContent: "center" }}>
              <span className="pill">
                <Radio size={13} /> {discoverable ? "Discoverable" : "Hidden"}
              </span>
              <span className="pill">
                <ShieldCheck size={13} /> End-to-end encrypted
              </span>
            </div>
            <NeonButton icon={Check} onClick={finish} loading={busy} block>
              Start using Nexo
            </NeonButton>
          </div>
        ) : null}
      </div>
    </div>
  );
}
