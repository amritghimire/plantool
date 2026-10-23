import {render,screen} from "@testing-library/react";
import {MemoryRouter,Routes,Route} from "react-router-dom";
import {it,expect,vi} from "vitest";
import {api} from "../api";
import {GithubPlan} from "./GithubPlan";
vi.mock("../api",()=>({api:{githubPlan:vi.fn()}}));
it("shows a published plan and discussion as read-only untrusted text",async()=>{
 vi.mocked(api.githubPlan).mockResolvedValue({gate:{number:1,hash:"a".repeat(64),state:"open"},summary:"Design discussion",plan:"<script>untrusted</script>",messages:[{role:"PM",body:"Scope is bounded"}]});
 render(<MemoryRouter initialEntries={["/github/plans/g1"]}><Routes><Route path="/github/plans/:id" element={<GithubPlan/>}/></Routes></MemoryRouter>);
 await screen.findByText("<script>untrusted</script>");expect(screen.getByText("Scope is bounded")).toBeInTheDocument();expect(screen.queryByRole("button",{name:/approve/i})).not.toBeInTheDocument();expect(api.githubPlan).toHaveBeenCalledWith("g1");
});
