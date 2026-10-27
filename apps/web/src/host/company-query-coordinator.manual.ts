import { numericCompanyCatalogFixture, defaultCatalogFixture } from "./catalog-test-fixture.ts";
import { CompanyQueryCoordinator } from "./company-query-coordinator.ts";
import { parseNormalizedEngineUpdate } from "./protocol/index.ts";
import { civilUpdate } from "./protocol-test-fixtures.ts";

const coordinator = new CompanyQueryCoordinator({
  capabilities: { deliveryModes: [], targetUiHz: 60, sharedMemory: false, reconnect: false, publicCompanyReports: false, npcDecisionDiagnostics: false },
}, (action) => console.log(action));
coordinator.installBaseline({ catalogResolver: numericCompanyCatalogFixture, civilDate: "2030-01-01", revision: "1", seq: 0 });
const update = parseNormalizedEngineUpdate(civilUpdate(), defaultCatalogFixture);
if (update.kind === "civil-update") coordinator.acceptCivil(update, { civilDate: "2030-01-03", revision: "2" });
