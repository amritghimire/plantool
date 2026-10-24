import {render,screen,fireEvent,waitFor} from "@testing-library/react";
import {it,expect,vi,beforeEach} from "vitest";
import {api} from "../api";
import {GithubSettings} from "./GithubSettings";
vi.mock("../api",()=>({api:{githubSettings:vi.fn(),configureGithub:vi.fn(),githubAllowlist:vi.fn(),githubPreflight:vi.fn()}}));
beforeEach(()=>{vi.clearAllMocks();vi.mocked(api.githubSettings).mockResolvedValue({repositories:[],allowlist:[{id:16842655,login:"amritghimire"}],sync:[]})});
it("shows numeric approvers and surfaces setup preflight warnings, clearing the key",async()=>{
 vi.mocked(api.configureGithub).mockResolvedValue({preflight:{warnings:["Require at least one approving review"]}});
 render(<GithubSettings/>);fireEvent.click(screen.getByText("GitHub App setup"));await screen.findByText("amritghimire (16842655)");
 for(const [label,value] of [["App ID","1"],["Installation ID","2"],["Repository (owner/name)","o/r"],["Project checkout (absolute path)","/repo"],["RSA private key (PEM; leave empty to reuse Keychain)","test-key"]])fireEvent.change(screen.getByLabelText(label),{target:{value}});
 fireEvent.click(screen.getByRole("button",{name:"Connect with passkey and run preflight"}));await screen.findByText("Require at least one approving review");expect(screen.getByLabelText("RSA private key (PEM; leave empty to reuse Keychain)")).toHaveValue("");
 expect(api.configureGithub).toHaveBeenCalledWith({app_id:1,installation_id:2,repo:"o/r",checkout:"/repo",private_key:"test-key"});
});
it("requires explicit approver changes and displays failures",async()=>{
 vi.mocked(api.githubAllowlist).mockRejectedValue(new Error("Cannot remove the last approver"));
 render(<GithubSettings/>);fireEvent.change(screen.getByLabelText("GitHub username"),{target:{value:"amritghimire"}});fireEvent.click(screen.getByRole("button",{name:"Remove approver with passkey"}));
 await waitFor(()=>expect(screen.getByRole("status")).toHaveTextContent("Cannot remove the last approver"));expect(api.githubAllowlist).toHaveBeenCalledWith("amritghimire",true);
});

it("keeps preflight warnings after reload and refreshes without re-entering App details",async()=>{
 const base={repositories:[{project:"p",repo_id:42,full_name:"o/r",visibility:"private",preflight:{warnings:["No required checks configured"]}}],allowlist:[],sync:[]};
 vi.mocked(api.githubSettings).mockResolvedValueOnce(base).mockResolvedValue({...base,repositories:[{...base.repositories[0],preflight:{warnings:[]}}]});
 vi.mocked(api.githubPreflight).mockResolvedValue({warnings:[]});
 render(<GithubSettings/>);fireEvent.click(screen.getByText("GitHub App setup"));await screen.findByText("No required checks configured");
 fireEvent.click(screen.getByRole("button",{name:"Refresh preflight with passkey for o/r"}));await screen.findByText("Preflight refreshed.");
 expect(api.githubPreflight).toHaveBeenCalledWith("p");expect(screen.queryByText("No required checks configured")).not.toBeInTheDocument();expect(api.configureGithub).not.toHaveBeenCalled();
});
