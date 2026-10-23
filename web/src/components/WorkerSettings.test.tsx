import {render,screen,fireEvent,waitFor} from "@testing-library/react";
import {it,expect,vi} from "vitest";
import {WorkerSettings} from "./WorkerSettings";
import {api} from "../api";
vi.mock("../api",()=>({api:{workerStatus:vi.fn(),repairWorker:vi.fn()}}));
it("shows repair failure without replacing Personal UI",async()=>{
 vi.mocked(api.workerStatus).mockResolvedValue({worker:{available:false,message:"Engineering OS unavailable"}});
 vi.mocked(api.repairWorker).mockRejectedValue(new Error("macOS sandbox-exec is required"));
 render(<WorkerSettings/>);fireEvent.click(screen.getByText("Engineering OS"));
 fireEvent.click(screen.getByRole("button",{name:"Repair with passkey"}));
 await waitFor(()=>expect(screen.getByRole("alert").textContent).toContain("macOS sandbox-exec"));
 expect(screen.getByRole("button",{name:"Repair with passkey"})).toBeEnabled();
});
