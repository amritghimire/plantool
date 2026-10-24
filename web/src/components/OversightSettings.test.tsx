import {render,screen,fireEvent,waitFor} from "@testing-library/react";
import {it,expect,vi} from "vitest";
import {api} from "../api";
import {OversightSettings} from "./OversightSettings";
vi.mock("../api",()=>({api:{oversight:vi.fn(),saveOversight:vi.fn(),saveReviewTags:vi.fn()}}));
it("edits Standard review tags against the pending hash without an approval action",async()=>{
 vi.mocked(api.oversight).mockResolvedValue({projects:[{id:"p",name:"owner/repo",version:1,preset:"standard",day:0,time:"09:00",timezone:"Asia/Kathmandu",review_globs:["**/auth/**"]}],plans:[{item:"p:1",title:"Auth change",state:{preset:"standard",plan_sha:"current-hash",phases:[{name:"Phase 1 [review]",tagged:true}]}}]});
 vi.mocked(api.saveReviewTags).mockResolvedValue({});render(<OversightSettings/>);fireEvent.click(screen.getByText("Refresh project policy"));
 const checkbox=await screen.findByRole("checkbox");expect((checkbox as HTMLInputElement).checked).toBe(true);fireEvent.click(checkbox);fireEvent.click(screen.getByText("Save review tags with passkey"));
 await waitFor(()=>expect(api.saveReviewTags).toHaveBeenCalledWith({item:"p:1",hash:"current-hash",tags:[false]}));
 expect(screen.queryByRole("button",{name:/approve/i})).toBeNull();
});
