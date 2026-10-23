import {render,screen,fireEvent,waitFor} from "@testing-library/react";
import {it,expect,vi} from "vitest";
import {api} from "../api";
import {ModelSettings} from "./ModelSettings";
vi.mock("../api",()=>({api:{modelSettings:vi.fn(),saveModels:vi.fn(),verifyModels:vi.fn()}}));
it("preserves an explicitly empty project provider allowlist",async()=>{
 vi.mocked(api.modelSettings).mockResolvedValue({yaml:"routes: {}",projects:[{id:"p1",checkout:"/repo",allowed_providers:["ollama"]}],routes:[]});
 vi.mocked(api.saveModels).mockResolvedValue({ok:true});
 render(<ModelSettings/>);fireEvent.click(screen.getByText("Model routes and providers"));
 await screen.findByDisplayValue("routes: {}");fireEvent.change(screen.getByLabelText("Project provider policy"),{target:{value:"p1"}});
 fireEvent.click(screen.getByLabelText("ollama"));fireEvent.click(screen.getByRole("button",{name:"Save chains with passkey"}));
 await waitFor(()=>expect(api.saveModels).toHaveBeenCalledWith({yaml:"routes: {}",project:"p1",allowed_providers:[]}));
});
