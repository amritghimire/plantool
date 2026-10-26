import {render,screen,fireEvent,waitFor,within} from "@testing-library/react";
import {MemoryRouter} from "react-router-dom";
import {api} from "../api";
import {IssueBoard,workflowColumn} from "./IssueBoard";
import type {EngineeringItem} from "../engineering";
const item:EngineeringItem={id:"p:1",project:"p",title:"Bounded task",issue:1,state:"plan_gate",step:"plan_gate",priority:null,pr:null,error:null,frozen:false,waiting_gates:1};
it("keeps actual approval status when a card is manually organized as done",async()=>{
 const move=vi.spyOn(api,"organizeItem").mockResolvedValue({column:"done"});const refresh=vi.fn();
 const {rerender}=render(<MemoryRouter><IssueBoard items={[item]} repository="o/r" refresh={refresh}/></MemoryRouter>);
 fireEvent.change(screen.getByRole("combobox",{name:"Column for Bounded task"}),{target:{value:"done"}});
 await waitFor(()=>expect(move).toHaveBeenCalledWith("p:1","done"));expect(refresh).toHaveBeenCalled();
 rerender(<MemoryRouter><IssueBoard items={[{...item,board_column:"done"}]} repository="o/r" refresh={refresh}/></MemoryRouter>);
 expect(within(screen.getByRole("region",{name:"Done"})).getByText("Bounded task")).toBeInTheDocument();
 expect(screen.getByText("Waiting for plan approval")).toBeInTheDocument();
 expect(screen.getByText(/Manual column/)).toHaveTextContent("Waiting for plan approval");
 fireEvent.change(screen.getByRole("combobox",{name:"Column for Bounded task"}),{target:{value:""}});
 await waitFor(()=>expect(move).toHaveBeenLastCalledWith("p:1",null));
});
it("uses real workflow columns and creates a scoped backlog issue link",()=>{
 expect(workflowColumn(item)).toBe("review");expect(workflowColumn({...item,step:"closed"})).toBe("done");expect(workflowColumn({...item,frozen:true})).toBe("waiting");
 render(<MemoryRouter><IssueBoard items={[]} repository="o/r" refresh={()=>{}}/></MemoryRouter>);
 fireEvent.change(screen.getByLabelText("New backlog issue"),{target:{value:"Fix & test"}});
 expect(screen.getByRole("link",{name:"Create issue on GitHub →"})).toHaveAttribute("href","https://github.com/o/r/issues/new?title=Fix%20%26%20test");
});
