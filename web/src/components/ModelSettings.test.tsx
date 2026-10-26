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

it("shows the failed capability and redacted diagnostic instead of requiring worker repair",async()=>{
 vi.mocked(api.modelSettings).mockResolvedValue({yaml:"routes: {}",projects:[],routes:[{route:"free",available:true,model:"example:free",error:null}]});
 Object.assign(api,{diagnoseModel:vi.fn().mockResolvedValue({route:"free",provider:"openrouter",model:"example:free",checked_at:1,ok:false,redacted:true,stages:[{stage:"tools",ok:false,message:"Provider rejected tools",next_step:"Use a tool-capable model",http_status:400}]})});
 render(<ModelSettings/>);fireEvent.click(screen.getByText("Model routes and providers"));
 fireEvent.click(await screen.findByRole("button",{name:"Test free with passkey"}));
 await screen.findByText("tools: failed");
 expect(screen.getByText("Next: Use a tool-capable model")).toBeInTheDocument();
 expect(screen.getByRole("button",{name:"Copy redacted diagnostics"})).toBeInTheDocument();
 expect(api.diagnoseModel).toHaveBeenCalledWith("free");
});

it("shows saved diagnostic failure beside the route and opens its details",async()=>{
 const diagnostic={route:"codex-sol",provider:"openai",model:"gpt-6.1-sol",checked_at:1,ok:false,redacted:true,stages:[{stage:"Codex CLI",ok:false,message:"Codex workspace routing discovery failed",next_step:"Check account routing"}]};
 vi.mocked(api.modelSettings).mockResolvedValue({yaml:"routes: {}",projects:[],routes:[{route:"codex-sol",available:true,model:"gpt-6.1-sol",error:null,diagnostic}]});
 render(<ModelSettings/>);fireEvent.click(screen.getByText("Model routes and providers"));
 expect(await screen.findByText(/Last check: failed/)).toHaveTextContent("Codex workspace routing discovery failed");
 fireEvent.click(screen.getByRole("button",{name:"Show last result for codex-sol"}));
 expect(await screen.findByText("Codex CLI: failed")).toBeInTheDocument();
 expect(screen.getByText("Next: Check account routing")).toBeInTheDocument();
});
