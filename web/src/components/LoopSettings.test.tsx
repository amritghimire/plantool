import {render,screen,fireEvent,waitFor} from "@testing-library/react";
import {it,expect,vi} from "vitest";
import {api} from "../api";
import {LoopSettings} from "./LoopSettings";
vi.mock("../api",()=>({api:{loop:vi.fn(),saveLoop:vi.fn(),overrideDecline:vi.fn()}}));
it("saves opt-in Nix and scan schedule with a version; urgency remains informational",async()=>{
 vi.mocked(api.loop).mockResolvedValue({projects:[{id:"p",name:"owner/repo",version:2,scan_day:6,scan_time:"08:00",nix_enabled:false}],declines:[{item:"p:1",title:"Scope mismatch",reason:"Not a fit",reset_at:"2026-10-24",active:true}],notifications:[{id:"n",body:"Urgency requested for #1",created_at:"today"}]});vi.mocked(api.saveLoop).mockResolvedValue({});vi.mocked(api.overrideDecline).mockResolvedValue({});render(<LoopSettings/>);fireEvent.click(screen.getByText("Refresh loop"));await screen.findByText("Urgency requested for #1");fireEvent.change(screen.getByLabelText("Loop project"),{target:{value:"p"}});fireEvent.click(screen.getByRole("checkbox"));fireEvent.click(screen.getByText("Save loop settings with passkey"));await waitFor(()=>expect(api.saveLoop).toHaveBeenCalledWith({project:"p",version:2,scan_day:6,scan_time:"08:00",nix_enabled:true}));fireEvent.click(screen.getByText("Return to triage with passkey"));await waitFor(()=>expect(api.overrideDecline).toHaveBeenCalledWith("p:1"));expect(screen.queryByRole("button",{name:/approve|promote/i})).toBeNull();
});
