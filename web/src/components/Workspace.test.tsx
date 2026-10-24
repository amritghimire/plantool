import {render,screen,fireEvent,waitFor} from "@testing-library/react";
import {it,expect,vi} from "vitest";
import {Workspace} from "./Workspace";
import {api} from "../api";
vi.mock("../api",()=>({api:{workspace:vi.fn(),workspaceReply:vi.fn()}}));
it("shows public threads read-only and sends owner text only as context",async()=>{
 vi.mocked(api.workspace).mockResolvedValue({projects:[{id:"p",name:"owner/repo"}],threads:[{id:"t",project:"p",item:null,channel:"general",title:"#general",messages:[{id:"m",sender:"cto",body:"Inspect contract",mentions:[],at:"now"}]}]});
 vi.mocked(api.workspaceReply).mockResolvedValue({id:"reply",authority:"context_only"});render(<Workspace/>);
 fireEvent.click(screen.getByText("Workspace"));fireEvent.click(screen.getByText("Refresh workspace"));
 fireEvent.click(await screen.findByRole("button",{name:"#general"}));expect(screen.getByText("Inspect contract")).toBeTruthy();
 fireEvent.change(screen.getByLabelText("Owner reply (context only)"),{target:{value:"approved"}});fireEvent.click(screen.getByText("Send context"));
 await waitFor(()=>expect(api.workspaceReply).toHaveBeenCalledWith(expect.objectContaining({thread:"t",body:"approved"})));
 expect(screen.queryByRole("button",{name:/approve/i})).toBeNull();
});
