import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const dialogSource = readFileSync(new URL("../EditorSettingsDialog.vue", import.meta.url), "utf8");

function sourceBlock(startMarker: string, endMarker: string): string {
  const start = dialogSource.indexOf(startMarker);
  const end = dialogSource.indexOf(endMarker, start);
  if (start < 0 || end < 0) throw new Error(`Missing source block: ${startMarker}`);
  return dialogSource.slice(start, end);
}

describe("EditorSettingsDialog CC-SWITCH provider", () => {
  it("shows CC-SWITCH only as a desktop built-in provider option", () => {
    expect(dialogSource).toContain('<SelectItem v-if="!isWeb" :value="CC_SWITCH_PROVIDER_ID">');
    expect(dialogSource).toContain("const aiIsCcSwitchProvider = computed(() => aiEditProviderPresetId.value === CC_SWITCH_PROVIDER_ID);");
  });

  it("does not install anything when the provider is selected", () => {
    const selectProvider = sourceBlock("function aiSelectProvider(presetId: string)", "\n}\n\nfunction aiSelectApiStyle");

    expect(selectProvider).toContain("CC_SWITCH_PROVIDER_ID");
    expect(selectProvider).not.toContain("aiInstallCcSwitchPlugin");
    expect(selectProvider).not.toContain("aiInstallCcSwitchPluginLocal");
  });

  it("keeps installation and import actions inside the selected provider panel", () => {
    const ccSwitchPanel = sourceBlock('<div v-if="aiIsCcSwitchProvider && !isWeb"', '\n                <!-- CLI MCP Status -->');

    expect(ccSwitchPanel).toContain('@click="aiInstallCcSwitchPlugin"');
    expect(ccSwitchPanel).toContain('@click="aiInstallCcSwitchPluginLocal"');
    expect(ccSwitchPanel).toContain('@click="aiImportCcSwitchConfigs"');
    expect(ccSwitchPanel).toContain("ai.ccSwitchImport");
  });

  it("removes CC-SWITCH actions from the config list toolbar", () => {
    const configListView = sourceBlock("<!-- Config List View -->", "<!-- Agent Turn Limit (list mode, global) -->");

    expect(configListView).not.toContain("aiInstallCcSwitchPlugin");
    expect(configListView).not.toContain("aiImportCcSwitchConfigs");
  });
});
